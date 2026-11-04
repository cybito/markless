use std::{
    ffi::OsString,
    fs,
    io::{self, BufRead, BufReader, Write},
    os::{
        fd::AsFd,
        unix::{
            ffi::OsStrExt,
            fs::{FileTypeExt, MetadataExt},
            net::UnixStream,
        },
    },
    path::{Component, Path, PathBuf},
    time::{Duration, Instant},
};

use anyhow::{Result, bail};
use serde::{Deserialize, Deserializer};
use serde_json::{Value, json};

const MAX_FRAME: usize = 4096;
const CONNECT_TIMEOUT: Duration = Duration::from_secs(1);
const RPC_TIMEOUT: Duration = Duration::from_secs(4);

pub(super) enum Transport {
    Direct(PathBuf),
    /// herdr-core terminal label; see imekit docs/protocol.md "reporter：hc transport".
    Hc {
        path: PathBuf,
        terminal: String,
    },
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) enum AckScope {
    Applied,
    Recorded,
    Inactive,
    Pending,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Response<'a> {
    ok: bool,
    generation: u64,
    #[serde(default, borrow, deserialize_with = "optional")]
    session: Option<&'a str>,
    #[serde(default, borrow, deserialize_with = "optional")]
    scope: Option<&'a str>,
    #[serde(default, borrow, deserialize_with = "optional")]
    error: Option<&'a str>,
}

/// Lifecycle request as produced by the reporter state machine.
#[derive(Deserialize)]
struct Lifecycle<'a> {
    op: &'a str,
    #[serde(default, borrow)]
    state: Option<&'a str>,
}

const HC_LABEL: &str = "imekit.intent";

/// hc transport state: the label writer keeps its own `held` flag.
struct Hc {
    terminal: String,
    pgid: i32,
    held: bool,
    next_id: u64,
    generation: u64,
}

// Optional fields may be absent, but an explicit null is not a protocol value.
fn optional<'de, D: Deserializer<'de>, T: Deserialize<'de>>(
    deserializer: D,
) -> std::result::Result<Option<T>, D::Error> {
    T::deserialize(deserializer).map(Some)
}

impl Transport {
    pub(super) fn select(
        lookup: impl Fn(&str) -> Option<OsString>,
        local_session: impl FnOnce() -> bool,
    ) -> Result<Option<Self>> {
        let terminal = lookup("HC_TERMINAL_ID");
        let socket = lookup("HC_TERMINAL_SOCKET");
        if terminal.is_some() || socket.is_some() {
            let (Some(terminal), Some(socket)) = (
                terminal.filter(|value| !value.is_empty()),
                socket.filter(|value| !value.is_empty()),
            ) else {
                bail!(
                    "HC_IME_INVALID_ENV: HC_TERMINAL_ID and HC_TERMINAL_SOCKET are both required"
                );
            };
            let terminal = terminal
                .into_string()
                .map_err(|_| anyhow::anyhow!("HC_IME_INVALID_ENV: invalid terminal identity"))?;
            let path = PathBuf::from(socket);
            validate_socket(&path, false)?;
            return Ok(Some(Self::Hc { path, terminal }));
        }
        if !local_session() {
            return Ok(None);
        }
        let home = lookup("HOME")
            .filter(|value| !value.is_empty())
            .ok_or_else(|| anyhow::anyhow!("HOME is unavailable for the IME control socket"))?;
        let home = PathBuf::from(home);
        if !home.is_absolute() {
            bail!("HOME is not absolute for the IME control socket");
        }
        Ok(Some(Self::Direct(home.join(
            ".local/state/infra-as-code/ime-control/run/control.sock",
        ))))
    }

    fn path(&self) -> &Path {
        match self {
            Self::Direct(path) | Self::Hc { path, .. } => path,
        }
    }
}

pub(super) struct Connection {
    stream: BufReader<UnixStream>,
    hc: Option<Hc>,
    session: Option<String>,
    generation: Option<u64>,
}

impl Connection {
    pub(super) fn connect(transport: &Transport) -> Result<Self> {
        let hc = matches!(transport, Transport::Hc { .. });
        let identity = validate_socket(transport.path(), !hc)?;
        let stream = connect(transport.path(), Instant::now() + CONNECT_TIMEOUT)?;
        if validate_socket(transport.path(), !hc)? != identity {
            bail!("IME_INVALID_PATH: socket was replaced during connect");
        }
        let mut connection = Self::new(stream);
        if let Transport::Hc { terminal, .. } = transport {
            connection.hc = Some(Hc::new(terminal.clone()));
        }
        Ok(connection)
    }

    fn new(stream: UnixStream) -> Self {
        Self {
            stream: BufReader::new(stream),
            hc: None,
            session: None,
            generation: None,
        }
    }

    pub(super) fn request(&mut self, request: &[u8]) -> Result<AckScope> {
        if self.hc.is_some() {
            let ack = self.hc_request(request)?;
            return self.acknowledgment(request, ack.as_bytes());
        }
        let deadline = Instant::now() + RPC_TIMEOUT;
        self.write(request, deadline)?;
        let (frame, length) = self.read(deadline)?;
        self.acknowledgment(request, &frame[..length])
    }

    /// Maps one lifecycle request onto the terminal label and returns the synthetic ACK.
    fn hc_request(&mut self, request: &[u8]) -> Result<String> {
        let lifecycle: Lifecycle = serde_json::from_slice(request)?;
        let hc = self.hc.as_mut().expect("hc transport");
        let state = || {
            lifecycle
                .state
                .filter(|state| matches!(*state, "command" | "text" | "idle"))
                .ok_or_else(|| anyhow::anyhow!("HC_IME_INVALID_REQUEST: missing state"))
        };
        let (value, held) = match lifecycle.op {
            "enter" => (Some("command"), true),
            "activate" | "resume" => (Some(state()?), true),
            "state" if hc.held => (Some(state()?), true),
            "state" => (None, false),
            "blur" | "suspend" => (Some("idle"), false),
            "close" => (Some(""), false),
            op => bail!("HC_IME_INVALID_REQUEST: unsupported op {op}"),
        };
        if let Some(value) = value {
            let label = if value.is_empty() {
                String::new()
            } else {
                format!("1:{}:{value}", hc.pgid)
            };
            self.label(&label)?;
        }
        let hc = self.hc.as_mut().expect("hc transport");
        hc.held = held;
        hc.generation += 1;
        Ok(json!({
            "ok": true,
            "generation": hc.generation,
            "session": format!("hc:{}", hc.terminal),
            "scope": "recorded",
        })
        .to_string())
    }

    fn label(&mut self, value: &str) -> Result<()> {
        let hc = self.hc.as_mut().expect("hc transport");
        hc.next_id += 1;
        let id = hc.next_id;
        let mut request = serde_json::to_vec(&json!({
            "jsonrpc": "2.0",
            "id": id,
            "method": "terminal.label",
            "params": { "terminal": hc.terminal, "labels": { HC_LABEL: value } },
        }))?;
        request.push(b'\n');
        let deadline = Instant::now() + RPC_TIMEOUT;
        self.write(&request, deadline)?;
        let (frame, length) = self.read(deadline)?;
        let reply: Value = serde_json::from_slice(&frame[..length])
            .map_err(|error| anyhow::anyhow!("HC_IME_BAD_REPLY: {error}"))?;
        if let Some(error) = reply.get("error") {
            bail!("HC_IME_REJECTED: {error}");
        }
        if reply.get("id").and_then(Value::as_u64) != Some(id) {
            bail!("HC_IME_BAD_REPLY: mismatched id");
        }
        let labels = reply
            .get("result")
            .and_then(Value::as_object)
            .and_then(|result| result.get("labels"))
            .and_then(Value::as_object)
            .ok_or_else(|| anyhow::anyhow!("HC_IME_BAD_REPLY: invalid result"))?;
        let recorded = labels.get(HC_LABEL);
        let valid = if value.is_empty() {
            recorded.is_none()
        } else {
            recorded.and_then(Value::as_str) == Some(value)
        };
        if !valid {
            bail!("HC_IME_BAD_REPLY: label was not recorded");
        }
        Ok(())
    }

    fn acknowledgment(&mut self, request: &[u8], frame: &[u8]) -> Result<AckScope> {
        let response: Response = serde_json::from_slice(frame)
            .map_err(|error| anyhow::anyhow!("IME_BAD_ACK: {error}"))?;
        if response.generation == 0
            || self
                .generation
                .is_some_and(|previous| response.generation < previous)
        {
            bail!("IME_BAD_ACK: generation regressed");
        }
        if !response.ok {
            if response.session.is_some() || response.scope.is_some() {
                bail!("IME_BAD_ACK: invalid rejection");
            }
            bail!(
                "IME request rejected: {}",
                response
                    .error
                    .filter(|error| !error.is_empty())
                    .ok_or_else(|| anyhow::anyhow!("IME_BAD_ACK: rejection omitted its error"))?
            );
        }
        if response.error.is_some() {
            bail!("IME_BAD_ACK: success contains an error");
        }
        let session = response
            .session
            .filter(|session| !session.is_empty())
            .ok_or_else(|| anyhow::anyhow!("IME_BAD_ACK: omitted session"))?;
        if self
            .session
            .as_deref()
            .is_some_and(|previous| previous != session)
        {
            bail!("IME_BAD_ACK: changed session on an existing connection");
        }
        let scope = match (self.hc.is_some(), response.scope) {
            (true, Some("recorded")) => AckScope::Recorded,
            (false, Some("applied")) => AckScope::Applied,
            (false, Some("inactive")) => AckScope::Inactive,
            (false, Some("pending")) if request == b"{\"op\":\"blur\"}\n" => AckScope::Pending,
            _ => bail!("IME_BAD_ACK: unexpected acknowledgment scope"),
        };
        if self.session.is_none() {
            self.session = Some(session.to_owned());
        }
        self.generation = Some(response.generation);
        Ok(scope)
    }

    fn write(&mut self, request: &[u8], deadline: Instant) -> Result<()> {
        if request.len() > MAX_FRAME || request.last() != Some(&b'\n') {
            bail!("IME request exceeds the frame limit or is incomplete");
        }
        let mut remaining = request;
        while !remaining.is_empty() {
            time_left(deadline)?;
            let count = match self.stream.get_mut().write(remaining) {
                Ok(count) => count,
                Err(error) if error.kind() == io::ErrorKind::Interrupted => continue,
                Err(error) if error.kind() == io::ErrorKind::WouldBlock => {
                    wait_ready(
                        self.stream.get_ref(),
                        rustix::event::PollFlags::OUT,
                        deadline,
                    )?;
                    continue;
                }
                Err(error) => return Err(error.into()),
            };
            if count == 0 {
                bail!("IME disconnected while writing request");
            }
            remaining = &remaining[count..];
        }
        Ok(())
    }

    fn read(&mut self, deadline: Instant) -> Result<([u8; MAX_FRAME], usize)> {
        let mut frame = [0u8; MAX_FRAME];
        let mut length = 0;
        loop {
            time_left(deadline)?;
            let available = match self.stream.fill_buf() {
                Ok(available) => available,
                Err(error) if error.kind() == io::ErrorKind::Interrupted => continue,
                Err(error) if error.kind() == io::ErrorKind::WouldBlock => {
                    wait_ready(
                        self.stream.get_ref(),
                        rustix::event::PollFlags::IN,
                        deadline,
                    )?;
                    continue;
                }
                Err(error) => return Err(error.into()),
            };
            if available.is_empty() {
                bail!("IME disconnected before ACK");
            }
            let count = available
                .iter()
                .position(|&byte| byte == b'\n')
                .map_or(available.len(), |end| end + 1);
            if length + count > MAX_FRAME {
                bail!("IME ACK exceeds 4096 bytes");
            }
            frame[length..length + count].copy_from_slice(&available[..count]);
            length += count;
            self.stream.consume(count);
            if frame[length - 1] == b'\n' {
                return Ok((frame, length));
            }
        }
    }

    /// Cached authorization is unusable once its lease socket has reached EOF.
    pub(super) fn alive(&self) -> Result<()> {
        if !self.stream.buffer().is_empty() {
            bail!("IME_BAD_ACK: unsolicited response");
        }
        let deadline = Instant::now() + CONNECT_TIMEOUT;
        loop {
            time_left(deadline)?;
            let mut byte = [0u8];
            let result = rustix::net::recv(
                self.stream.get_ref(),
                &mut byte[..],
                rustix::net::RecvFlags::PEEK,
            );
            let error = match result {
                Ok((_, 0)) => {
                    bail!("IME disconnected; cached command acknowledgment is no longer valid")
                }
                Ok(_) => bail!("IME_BAD_ACK: unsolicited response"),
                Err(error) => io::Error::from(error),
            };
            if error.kind() == io::ErrorKind::WouldBlock {
                return Ok(());
            }
            if error.kind() != io::ErrorKind::Interrupted {
                return Err(error.into());
            }
        }
    }

    #[cfg(test)]
    pub(super) fn from_stream(stream: UnixStream) -> Self {
        stream
            .set_nonblocking(true)
            .expect("IME test stream must support nonblocking I/O");
        Self::new(stream)
    }
}

impl Hc {
    fn new(terminal: String) -> Self {
        Self {
            terminal,
            pgid: rustix::process::getpgrp().as_raw_nonzero().get(),
            held: false,
            next_id: 0,
            generation: 0,
        }
    }
}

fn time_left(deadline: Instant) -> Result<Duration> {
    deadline
        .checked_duration_since(Instant::now())
        .filter(|remaining| !remaining.is_zero())
        .ok_or_else(|| anyhow::anyhow!("IME_ACK_TIMEOUT: complete request deadline expired"))
}

fn wait_ready(fd: &impl AsFd, events: rustix::event::PollFlags, deadline: Instant) -> Result<()> {
    loop {
        let remaining = time_left(deadline)?;
        let timeout = rustix::event::Timespec::try_from(remaining)?;
        let mut descriptors = [rustix::event::PollFd::new(fd, events)];
        match rustix::event::poll(&mut descriptors, Some(&timeout)) {
            Ok(0) | Err(rustix::io::Errno::INTR) => {}
            Ok(_) => {
                if descriptors[0]
                    .revents()
                    .contains(rustix::event::PollFlags::NVAL)
                {
                    bail!("IME socket is invalid");
                }
                return Ok(());
            }
            Err(error) => return Err(error.into()),
        }
    }
}

/// Returns the socket identity so callers can detect replacement across connect.
/// herdr-core leaves its socket 0755 inside a private directory, so only the direct
/// daemon socket must itself be private.
fn validate_socket(path: &Path, private_socket: bool) -> Result<(u64, u64)> {
    let uid = rustix::process::geteuid().as_raw();
    if !path.is_absolute()
        || path
            .as_os_str()
            .as_bytes()
            .split(|&byte| byte == b'/')
            .any(|part| part == b"." || part == b"..")
    {
        bail!("IME_INVALID_PATH: socket path must be absolute and normalized");
    }
    let mut ancestor = PathBuf::new();
    let mut identity = None;
    for component in path.components() {
        if !matches!(component, Component::RootDir | Component::Normal(_)) {
            bail!("IME_INVALID_PATH: invalid socket path component");
        }
        ancestor.push(component.as_os_str());
        let info = fs::symlink_metadata(&ancestor)?;
        if info.file_type().is_symlink() {
            bail!("IME_INVALID_PATH: symlink traversal is forbidden");
        }
        if ancestor == path {
            if !info.file_type().is_socket()
                || info.uid() != uid
                || (private_socket && info.mode() & 0o077 != 0)
            {
                bail!("IME_INVALID_PATH: socket must be owned by the current UID");
            }
            identity = Some((info.dev(), info.ino()));
        } else {
            if !info.is_dir()
                || (info.uid() != uid && info.uid() != 0)
                || (info.mode() & 0o022 != 0 && !(info.uid() == 0 && info.mode() & 0o1000 != 0))
            {
                bail!("IME_INVALID_PATH: unsafe socket ancestor");
            }
            if Some(&*ancestor) == path.parent() && (info.uid() != uid || info.mode() & 0o077 != 0)
            {
                bail!(
                    "IME_INVALID_PATH: socket parent must be private and owned by the current UID"
                );
            }
        }
    }
    identity.ok_or_else(|| anyhow::anyhow!("IME_INVALID_PATH: socket path has no target"))
}

fn connect(path: &Path, deadline: Instant) -> Result<UnixStream> {
    use rustix::net::{AddressFamily, SocketAddrUnix, SocketFlags, SocketType, socket_with};
    let address = SocketAddrUnix::new(path)?;
    let socket = socket_with(
        AddressFamily::UNIX,
        SocketType::STREAM,
        SocketFlags::empty(),
        None,
    )?;
    // macOS has no atomic SOCK_CLOEXEC/SOCK_NONBLOCK socket flags.
    rustix::io::fcntl_setfd(&socket, rustix::io::FdFlags::CLOEXEC)?;
    let flags = rustix::fs::fcntl_getfl(&socket)?;
    rustix::fs::fcntl_setfl(&socket, flags | rustix::fs::OFlags::NONBLOCK)?;
    match rustix::net::connect(&socket, &address) {
        Ok(()) => {}
        Err(rustix::io::Errno::INPROGRESS | rustix::io::Errno::INTR) => {
            wait_ready(&socket, rustix::event::PollFlags::OUT, deadline)?;
            rustix::net::sockopt::socket_error(&socket)??;
        }
        Err(error) => return Err(error.into()),
    }
    Ok(UnixStream::from(socket))
}

#[cfg(test)]
mod tests {
    use std::{
        io::Write,
        os::unix::net::UnixStream,
        sync::mpsc,
        thread,
        time::{Duration, Instant},
    };

    use anyhow::Result;

    use super::Connection;

    #[test]
    fn complete_ack_remains_readable_after_peer_closes() -> Result<()> {
        let (client, mut peer) = UnixStream::pair()?;
        peer.write_all(
            b"{\"ok\":true,\"generation\":1,\"session\":\"final-lease\",\"scope\":\"applied\"}\n",
        )?;
        drop(peer);
        let mut connection = Connection::from_stream(client);
        let (frame, length) = connection.read(Instant::now() + Duration::from_millis(80))?;
        let response: serde_json::Value = serde_json::from_slice(&frame[..length])?;
        assert_eq!(response["session"], "final-lease");
        assert_eq!(response["scope"], "applied");
        assert!(connection.alive().is_err());
        Ok(())
    }

    #[test]
    fn fragmented_ack_does_not_extend_the_complete_frame_deadline() -> Result<()> {
        let (client, mut peer) = UnixStream::pair()?;
        let (started, ready) = mpsc::channel();
        let service = thread::spawn(move || {
            peer.write_all(b"{").unwrap();
            started.send(()).unwrap();
            for _ in 0..100 {
                thread::sleep(Duration::from_millis(10));
                if peer.write_all(b" ").is_err() {
                    return;
                }
            }
            peer.write_all(b"}\n").ok();
        });
        ready.recv()?;
        let mut connection = Connection::from_stream(client);
        assert!(
            connection
                .read(Instant::now() + Duration::from_millis(80))
                .is_err()
        );
        drop(connection);
        service.join().unwrap();
        Ok(())
    }
}
