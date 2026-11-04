use std::{
    ffi::OsString,
    fs,
    io::{BufRead, BufReader, Write},
    os::unix::{
        fs::{DirBuilderExt, PermissionsExt, symlink},
        net::{UnixListener, UnixStream},
    },
    path::PathBuf,
    sync::{
        atomic::{AtomicU64, Ordering},
        mpsc,
    },
    thread,
    time::Duration,
};

use anyhow::Result;
use serde_json::{Value, json};

use super::{
    Ime,
    transport::{Connection, Transport},
};

fn reporter(client: UnixStream) -> Ime {
    Ime {
        enabled: true,
        running: true,
        focused: false,
        focus_lost: false,
        suspended: false,
        state: None,
        transport: None,
        connection: Some(Connection::from_stream(client)),
    }
}

fn assert_disabled(ime: &mut Ime) {
    assert!(!ime.enabled);
    assert!(ime.connection.is_none());
    assert!(ime.transport.is_none());
    assert!(
        ime.state.is_none(),
        "ordinary input is not an applied or recorded ACK"
    );
    assert!(!ime.focused);
    assert!(
        ime.command_ready(),
        "disabled reporter must dispatch the current command key"
    );
    ime.sync(true);
    ime.focus_in(true);
    ime.input(false);
    ime.input(true);
    ime.focus_out();
    ime.stop();
    assert!(!ime.running);
    ime.resume(false);
    assert!(ime.running);
    ime.input(false);
    ime.quit();
    assert!(ime.state.is_none());
    assert!(ime.connection.is_none());
    assert!(
        !ime.enabled,
        "no lifecycle event may reconnect or replay a failed reporter"
    );
}

#[test]
fn missing_service_disables_reporter_without_blocking_input_or_lifecycle() -> Result<()> {
    let fixture = Fixture::new()?;
    let mut ime = Ime {
        enabled: true,
        running: true,
        focused: false,
        focus_lost: false,
        suspended: false,
        state: None,
        transport: Some(Transport::Direct(fixture.root.join("missing"))),
        connection: None,
    };
    ime.focus_in(false);
    assert_disabled(&mut ime);
    // A subsequently available socket cannot revive the reporter in this process.
    let service = UnixListener::bind(fixture.root.join("missing"))?;
    service.set_nonblocking(true)?;
    ime.resume(false);
    ime.input(false);
    assert_disabled(&mut ime);
    assert_eq!(
        service.accept().unwrap_err().kind(),
        std::io::ErrorKind::WouldBlock
    );
    Ok(())
}

fn receive(peer: &mut BufReader<UnixStream>, op: &str, state: Option<&str>) {
    let mut frame = String::new();
    peer.read_line(&mut frame).unwrap();
    let request: Value = serde_json::from_str(&frame).unwrap();
    assert_eq!(request["op"], op);
    assert_eq!(request.get("state").and_then(Value::as_str), state);
    if op == "activate" {
        assert_eq!(request["policy"], "mode");
    }
}

fn acknowledge(peer: &mut BufReader<UnixStream>, generation: u64, scope: &str) {
    writeln!(
        peer.get_mut(),
        "{}",
        json!({ "ok": true, "generation": generation, "session": "test-lease", "scope": scope })
    )
    .unwrap();
}

#[test]
fn reports_mode_focus_and_lifetime_without_background_reactivation() -> Result<()> {
    let (client, server) = UnixStream::pair()?;
    let mut ime = reporter(client);
    let service = thread::spawn(move || {
        let mut peer = BufReader::new(server);
        for (op, state, scope) in [
            ("activate", Some("command"), "applied"),
            ("resume", Some("command"), "applied"),
            ("state", Some("text"), "applied"),
            ("blur", None, "pending"),
            ("activate", Some("text"), "applied"),
            ("suspend", None, "applied"),
            ("activate", Some("text"), "applied"),
            ("resume", Some("command"), "applied"),
            ("close", None, "applied"),
        ] {
            receive(&mut peer, op, state);
            acknowledge(&mut peer, 1, scope);
        }
    });

    ime.sync(false);
    assert!(!ime.command_ready());
    ime.focus_in(false);
    assert!(ime.command_ready());
    ime.sync(false);
    ime.input(false);
    assert!(ime.command_ready());
    ime.sync(true);
    assert!(!ime.command_ready());
    ime.focus_out();
    assert!(!ime.command_ready());
    ime.sync(false);
    ime.focus_in(true);
    ime.stop();
    ime.sync(false);
    ime.input(false);
    assert!(!ime.command_ready());
    ime.resume(true);
    assert!(!ime.command_ready());
    ime.sync(true);
    ime.input(false);
    assert!(ime.command_ready());
    ime.quit();
    assert!(!ime.command_ready());
    service.join().unwrap();
    Ok(())
}

#[test]
fn command_key_cannot_be_authorized_before_service_ack() -> Result<()> {
    let (client, server) = UnixStream::pair()?;
    let (done, verdict) = mpsc::channel();
    let worker = thread::spawn(move || {
        let mut ime = reporter(client);
        assert!(!ime.command_ready());
        ime.input(false);
        done.send(ime.command_ready()).unwrap();
    });
    let mut peer = BufReader::new(server);
    receive(&mut peer, "activate", Some("command"));
    assert!(verdict.recv_timeout(Duration::from_millis(50)).is_err());
    acknowledge(&mut peer, 1, "applied");
    assert!(verdict.recv_timeout(Duration::from_secs(1))?);
    worker.join().unwrap();
    Ok(())
}

#[test]
fn inactive_cancels_command_until_real_input_samples_current_editing() -> Result<()> {
    let (client, server) = UnixStream::pair()?;
    let mut ime = reporter(client);
    let service = thread::spawn(move || {
        let mut peer = BufReader::new(server);
        for (op, state, scope) in [
            ("activate", Some("command"), "inactive"),
            ("activate", Some("text"), "applied"),
            ("state", Some("command"), "inactive"),
            ("activate", Some("command"), "applied"),
            ("close", None, "applied"),
        ] {
            receive(&mut peer, op, state);
            acknowledge(&mut peer, 1, scope);
        }
    });
    ime.focus_in(false);
    assert!(!ime.command_ready());
    ime.sync(false);
    ime.sync(true);
    ime.input(true);
    assert!(!ime.command_ready());
    ime.sync(false);
    assert!(!ime.command_ready());
    ime.sync(true);
    ime.input(false);
    assert!(ime.command_ready());
    ime.quit();
    service.join().unwrap();
    Ok(())
}

#[test]
fn rejected_malformed_or_incomplete_ack_revokes_protection_without_retry() -> Result<()> {
    let mut oversized = vec![b'x'; 4096];
    oversized.push(b'\n');
    for reply in [
		b"{\"ok\":false,\"generation\":2,\"error\":\"BACKEND_UNAVAILABLE\"}\n".as_slice(),
		b"{\"ok\":false,\"generation\":2,\"error\":\"FOCUS_UNVERIFIED\"}\n".as_slice(),
		b"{\"ok\":true,\"generation\":3}\n".as_slice(),
		b"{\"ok\":true,\"generation\":1,\"session\":\"test-lease\"}\n".as_slice(),
		b"{\"ok\":true,\"generation\":1,\"session\":\"test-lease\",\"scope\":\"recorded\"}\n".as_slice(),
		b"{\"ok\":true,\"generation\":1,\"session\":\"test-lease\",\"scope\":\"pending\"}\n".as_slice(),
		b"{\"ok\":true,\"generation\":true,\"session\":\"test-lease\",\"scope\":\"applied\"}\n".as_slice(),
		b"{\"ok\":true,\"generation\":1,\"session\":\"test-lease\",\"scope\":\"applied\",\"error\":\"UNKNOWN\"}\n".as_slice(),
		b"{\"ok\":true,\"generation\":1,\"session\":\"test-lease\",\"scope\":\"applied\",\"extra\":true}\n".as_slice(),
		b"{\"ok\":true,\"generation\":1,\"session\":\"test-lease\",\"scope\":\"applied\",\"error\":null}\n".as_slice(),
		b"{\"ok\":true".as_slice(),
		oversized.as_slice(),
	] {
		let (client, server) = UnixStream::pair()?;
		let mut ime = reporter(client);
		let reply = reply.to_owned();
		let service = thread::spawn(move || {
			let mut peer = BufReader::new(server);
			receive(&mut peer, "activate", Some("command"));
			peer.get_mut().write_all(&reply).unwrap();
		});
		ime.input(false);
		assert_disabled(&mut ime);
		service.join().unwrap();
	}
    Ok(())
}

#[test]
fn stable_session_and_monotonic_generation_are_required_across_modes() -> Result<()> {
    for reply in [
        json!({ "ok": true, "generation": 2, "session": "different", "scope": "applied" }),
        json!({ "ok": true, "generation": 1, "session": "test-lease", "scope": "applied" }),
        json!({ "ok": true, "generation": -1, "session": "test-lease", "scope": "applied" }),
    ] {
        let (client, server) = UnixStream::pair()?;
        let mut ime = reporter(client);
        let service = thread::spawn(move || {
            let mut peer = BufReader::new(server);
            receive(&mut peer, "activate", Some("command"));
            acknowledge(&mut peer, 2, "applied");
            receive(&mut peer, "state", Some("text"));
            writeln!(peer.get_mut(), "{reply}").unwrap();
        });
        ime.focus_in(false);
        assert!(ime.command_ready());
        ime.sync(true);
        assert_disabled(&mut ime);
        service.join().unwrap();
    }
    Ok(())
}

#[test]
fn failed_suspend_allows_handoff_and_never_replays_uncertain_source_state() -> Result<()> {
    for reply in [
        json!({ "ok": false, "generation": 2, "error": "RESTORE_FAILED" }),
        json!({ "ok": true, "generation": 2, "session": "test-lease", "scope": "inactive" }),
    ] {
        let (client, server) = UnixStream::pair()?;
        let mut ime = reporter(client);
        let service = thread::spawn(move || {
            let mut peer = BufReader::new(server);
            receive(&mut peer, "activate", Some("command"));
            acknowledge(&mut peer, 1, "applied");
            receive(&mut peer, "suspend", None);
            writeln!(peer.get_mut(), "{reply}").unwrap();
        });
        ime.focus_in(false);
        ime.stop();
        assert!(
            !ime.running,
            "optional reporter must not cancel terminal handoff"
        );
        assert_disabled(&mut ime);
        service.join().unwrap();
    }
    Ok(())
}

struct Fixture {
    root: PathBuf,
    path: PathBuf,
    listener: UnixListener,
}

const TERMINAL: &str = "term-7";

impl Fixture {
    fn new() -> Result<Self> {
        static NEXT: AtomicU64 = AtomicU64::new(0);
        let root = std::env::temp_dir().canonicalize()?.join(format!(
            "markless-ime-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        fs::DirBuilder::new().mode(0o700).create(&root)?;
        let path = root.join("s");
        let listener = UnixListener::bind(&path)?;
        // herdr-core leaves its socket 0755 inside the private directory.
        fs::set_permissions(&path, fs::Permissions::from_mode(0o755))?;
        Ok(Self {
            root,
            path,
            listener,
        })
    }

    fn env(&self, socket: &std::path::Path) -> Vec<(&'static str, OsString)> {
        vec![
            ("HC_TERMINAL_ID", OsString::from(TERMINAL)),
            ("HC_TERMINAL_SOCKET", socket.as_os_str().to_owned()),
            // hc selection precedes and ignores SSH/GUI eligibility.
            ("SSH_CONNECTION", OsString::from("1 2 3 4")),
        ]
    }

    fn select_with(&self, socket: &std::path::Path) -> Result<Option<Transport>> {
        let values = self.env(socket);
        Transport::select(
            |key| {
                values
                    .iter()
                    .find(|(name, _)| *name == key)
                    .map(|(_, value)| value.clone())
            },
            || panic!("hc must be selected before ordinary SSH/GUI eligibility"),
        )
    }

    fn reporter(&self) -> Result<Ime> {
        Ok(Ime {
            enabled: true,
            running: true,
            focused: false,
            focus_lost: false,
            suspended: false,
            state: None,
            transport: Some(self.select_with(&self.path)?.unwrap()),
            connection: None,
        })
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        fs::remove_dir_all(&self.root).unwrap();
    }
}

/// Reads one `terminal.label` write and returns its id.
fn label_request(peer: &mut BufReader<UnixStream>, state: Option<&str>) -> Value {
    let mut frame = String::new();
    assert!(
        peer.read_line(&mut frame).unwrap() > 0,
        "expected a label write"
    );
    let request: Value = serde_json::from_str(&frame).unwrap();
    assert_eq!(request["jsonrpc"], "2.0");
    assert_eq!(request["method"], "terminal.label");
    assert_eq!(request["params"]["terminal"], TERMINAL);
    let expected = state.map_or_else(String::new, |state| {
        format!("1:{}:{state}", rustix::process::getpgrp().as_raw_nonzero())
    });
    assert_eq!(request["params"]["labels"]["imekit.intent"], expected);
    assert_eq!(request["params"]["labels"].as_object().unwrap().len(), 1);
    request
}

/// Accepts one label write as herdr-core would.
fn label(peer: &mut BufReader<UnixStream>, state: Option<&str>) {
    let request = label_request(peer, state);
    let value = &request["params"]["labels"]["imekit.intent"];
    let labels = if value == "" {
        json!({ "other": "x" })
    } else {
        json!({ "imekit.intent": value, "other": "x" })
    };
    writeln!(
        peer.get_mut(),
        "{}",
        json!({ "jsonrpc": "2.0", "id": request["id"], "result": { "terminal": TERMINAL, "labels": labels } })
    )
    .unwrap();
}

fn assert_no_more_writes(mut peer: BufReader<UnixStream>) {
    let mut unexpected = String::new();
    assert_eq!(peer.read_line(&mut unexpected).unwrap(), 0);
}

#[test]
fn hc_labels_follow_lifecycle_and_preserve_parent_handoff() -> Result<()> {
    let fixture = Fixture::new()?;
    let mut ime = fixture.reporter()?;
    let listener = fixture.listener.try_clone()?;
    let service = thread::spawn(move || {
        let mut peer = BufReader::new(listener.accept().unwrap().0);
        for state in [
            Some("command"), // activate
            Some("text"),    // state
            Some("idle"),    // suspend
            Some("text"),    // resume activate
            Some("command"), // state
            Some("idle"),    // blur
            Some("text"),    // activate
            None,            // close deletes
        ] {
            label(&mut peer, state);
        }
        assert_no_more_writes(peer);
    });
    ime.focus_in(false);
    assert!(ime.command_ready());
    ime.sync(true);
    ime.stop();
    ime.sync(false);
    assert!(!ime.command_ready());
    ime.resume(true);
    ime.sync(true);
    assert!(!ime.command_ready());
    ime.input(false);
    assert!(ime.command_ready());
    ime.focus_out();
    ime.sync(false);
    assert!(!ime.command_ready());
    ime.input(true);
    assert!(!ime.focused, "input must not override explicit focus loss");
    ime.focus_in(true);
    ime.quit();
    assert!(ime.enabled);
    service.join().unwrap();
    Ok(())
}

#[test]
fn hc_state_while_not_held_writes_nothing_and_acks_are_synthetic() -> Result<()> {
    let fixture = Fixture::new()?;
    let transport = fixture.select_with(&fixture.path)?.unwrap();
    let listener = fixture.listener.try_clone()?;
    let service = thread::spawn(move || {
        let mut peer = BufReader::new(listener.accept().unwrap().0);
        label(&mut peer, Some("command"));
        label(&mut peer, Some("idle"));
        label(&mut peer, Some("text"));
        label(&mut peer, None);
        assert_no_more_writes(peer);
    });
    let mut connection = Connection::connect(&transport)?;
    let recorded = Some(super::transport::AckScope::Recorded);
    for request in [
        &b"{\"op\":\"state\",\"state\":\"text\"}\n"[..],
        b"{\"op\":\"enter\"}\n",
        b"{\"op\":\"blur\"}\n",
        b"{\"op\":\"state\",\"state\":\"command\"}\n",
        b"{\"op\":\"resume\",\"state\":\"text\"}\n",
        b"{\"op\":\"close\"}\n",
    ] {
        assert_eq!(connection.request(request).ok(), recorded);
    }
    drop(connection);
    service.join().unwrap();
    Ok(())
}

#[test]
fn terminal_return_reports_own_current_mode_before_any_input() -> Result<()> {
    for editing in [false, true] {
        let fixture = Fixture::new()?;
        let mut ime = fixture.reporter()?;
        let listener = fixture.listener.try_clone()?;
        let service = thread::spawn(move || {
            let mut peer = BufReader::new(listener.accept().unwrap().0);
            label(&mut peer, Some("command"));
            label(&mut peer, Some("idle"));
            label(&mut peer, Some(if editing { "text" } else { "command" }));
            label(&mut peer, None);
            assert_no_more_writes(peer);
        });
        ime.focus_in(false);
        ime.stop();
        ime.sync(editing);
        assert!(!ime.command_ready());
        ime.resume(editing);
        assert_eq!(ime.command_ready(), !editing);
        ime.sync(editing);
        ime.quit();
        service.join().unwrap();
    }
    Ok(())
}

#[test]
fn inactive_terminal_return_cannot_authorize_or_replay_a_mode() -> Result<()> {
    let (client, server) = UnixStream::pair()?;
    let mut ime = reporter(client);
    let service = thread::spawn(move || {
        let mut peer = BufReader::new(server);
        for (op, state, scope) in [
            ("activate", Some("command"), "applied"),
            ("suspend", None, "applied"),
            ("activate", Some("command"), "inactive"),
            ("close", None, "applied"),
        ] {
            receive(&mut peer, op, state);
            acknowledge(&mut peer, 1, scope);
        }
    });
    ime.focus_in(false);
    ime.stop();
    ime.resume(false);
    assert!(ime.enabled);
    assert!(!ime.command_ready());
    ime.sync(true);
    ime.sync(false);
    assert!(!ime.command_ready());
    ime.quit();
    service.join().unwrap();
    Ok(())
}

#[test]
fn invalid_hc_reply_disables_reporter() -> Result<()> {
    let pgid = rustix::process::getpgrp().as_raw_nonzero().get();
    let good = json!({ "imekit.intent": format!("1:{pgid}:command") });
    for reply in [
        json!({ "jsonrpc": "2.0", "id": 1, "error": { "code": -32602, "message": "no terminal" } }),
        json!({ "jsonrpc": "2.0", "id": 1, "result": { "labels": good }, "error": null }),
        json!({ "jsonrpc": "2.0", "id": 2, "result": { "labels": good } }),
        json!({ "jsonrpc": "2.0", "id": 1, "result": { "labels": { "imekit.intent": "1:text" } } }),
        json!({ "jsonrpc": "2.0", "id": 1, "result": { "labels": {} } }),
        json!({ "jsonrpc": "2.0", "id": 1, "result": "ok" }),
        json!({ "jsonrpc": "2.0", "id": 1, "result": { "labels": [] } }),
    ] {
        let fixture = Fixture::new()?;
        let mut ime = fixture.reporter()?;
        let listener = fixture.listener.try_clone()?;
        let service = thread::spawn(move || {
            let mut peer = BufReader::new(listener.accept().unwrap().0);
            label_request(&mut peer, Some("command"));
            writeln!(peer.get_mut(), "{reply}").unwrap();
            assert_no_more_writes(peer);
        });
        ime.focus_in(false);
        assert_disabled(&mut ime);
        service.join().unwrap();
    }
    Ok(())
}

#[test]
fn partial_hc_env_never_falls_back_to_the_host_daemon() -> Result<()> {
    for values in [
        vec![("HC_TERMINAL_ID", "t")],
        vec![("HC_TERMINAL_SOCKET", "/tmp/s")],
        vec![("HC_TERMINAL_ID", ""), ("HC_TERMINAL_SOCKET", "/tmp/s")],
        vec![("HC_TERMINAL_ID", "t"), ("HC_TERMINAL_SOCKET", "")],
        vec![
            ("HC_TERMINAL_ID", "t"),
            ("HC_TERMINAL_SOCKET", "tcp://host:123"),
        ],
        vec![
            ("HC_TERMINAL_ID", "t"),
            ("HC_TERMINAL_SOCKET", "/tmp/../tmp/s"),
        ],
    ] {
        assert!(
            Transport::select(
                |key| values
                    .iter()
                    .find(|(name, _)| *name == key)
                    .map(|(_, value)| OsString::from(value)),
                || panic!("invalid hc env must never reach direct eligibility")
            )
            .is_err()
        );
    }
    assert!(Transport::select(|_| None, || false)?.is_none());
    Ok(())
}

#[test]
fn hc_refuses_shared_parent_foreign_target_and_symlink_traversal() -> Result<()> {
    let fixture = Fixture::new()?;
    fixture.listener.set_nonblocking(true)?;
    fs::set_permissions(&fixture.root, fs::Permissions::from_mode(0o755))?;
    assert!(fixture.select_with(&fixture.path).is_err());
    fs::set_permissions(&fixture.root, fs::Permissions::from_mode(0o700))?;
    let alias = fixture.root.join("alias");
    symlink(&fixture.path, &alias)?;
    assert!(fixture.select_with(&alias).is_err());
    let plain = fixture.root.join("plain");
    fs::write(&plain, b"")?;
    assert!(fixture.select_with(&plain).is_err());
    // Even once selected, a parent made unsafe before connect refuses the socket.
    let mut ime = fixture.reporter()?;
    fs::set_permissions(&fixture.root, fs::Permissions::from_mode(0o750))?;
    ime.focus_in(false);
    fs::set_permissions(&fixture.root, fs::Permissions::from_mode(0o700))?;
    assert_disabled(&mut ime);
    assert_eq!(
        fixture.listener.accept().unwrap_err().kind(),
        std::io::ErrorKind::WouldBlock
    );
    Ok(())
}

#[test]
fn eof_after_command_ack_does_not_reconnect_or_replay_the_old_mode() -> Result<()> {
    let (client, server) = UnixStream::pair()?;
    let mut ime = reporter(client);
    let service = thread::spawn(move || {
        let mut peer = BufReader::new(server);
        receive(&mut peer, "activate", Some("command"));
        acknowledge(&mut peer, 1, "applied");
        receive(&mut peer, "state", Some("text"));
    });
    ime.focus_in(false);
    assert!(ime.command_ready());
    ime.sync(true);
    assert_disabled(&mut ime);
    service.join().unwrap();
    Ok(())
}

#[test]
fn same_mode_key_cannot_use_a_cached_ack_after_socket_eof() -> Result<()> {
    let (client, server) = UnixStream::pair()?;
    let mut ime = reporter(client);
    let service = thread::spawn(move || {
        let mut peer = BufReader::new(server);
        receive(&mut peer, "activate", Some("command"));
        acknowledge(&mut peer, 1, "applied");
    });
    ime.focus_in(false);
    service.join().unwrap();
    ime.input(false);
    assert_disabled(&mut ime);
    Ok(())
}

#[test]
fn real_key_checks_direct_focus_even_when_no_focus_lost_event_was_reported() -> Result<()> {
    let (client, server) = UnixStream::pair()?;
    let mut ime = reporter(client);
    let service = thread::spawn(move || {
        let mut peer = BufReader::new(server);
        receive(&mut peer, "activate", Some("command"));
        acknowledge(&mut peer, 1, "applied");
        receive(&mut peer, "resume", Some("command"));
        acknowledge(&mut peer, 2, "inactive");
        receive(&mut peer, "activate", Some("text"));
        acknowledge(&mut peer, 3, "applied");
        receive(&mut peer, "close", None);
        acknowledge(&mut peer, 4, "applied");
    });
    ime.focus_in(false);
    assert!(ime.command_ready());
    ime.input(false);
    assert!(!ime.command_ready());
    ime.sync(false);
    ime.input(true);
    assert!(!ime.command_ready());
    ime.quit();
    service.join().unwrap();
    Ok(())
}

fn model() -> crate::app::Model {
    crate::app::Model::new(
        PathBuf::from("test.md"),
        crate::document::Document::parse("# Test").unwrap(),
        (80, 24),
    )
}

#[test]
fn classifier_matches_input_precedence_and_persistent_search() {
    use crate::app::{Message, update};
    let mut model = model();
    assert!(!super::text_mode(&model));
    model = update(model, Message::StartSearch);
    assert!(super::text_mode(&model));
    model = update(model, Message::NextMatch);
    assert!(super::text_mode(&model));
    model.help_visible = true;
    assert!(!super::text_mode(&model));
    model.help_visible = false;
    model = update(model, Message::ClearSearch);
    assert!(!super::text_mode(&model));
    model.editor_mode = true;
    assert!(super::text_mode(&model));
    model.link_picker_items.push(crate::document::LinkRef {
        text: "link".into(),
        url: "https://example.com".into(),
        line: 0,
    });
    assert!(!super::text_mode(&model));
}

#[test]
fn event_gate_waits_for_current_mode_and_background_does_not_activate() -> Result<()> {
    use crossterm::event::{Event, KeyCode, KeyEvent, KeyModifiers};
    let (client, server) = UnixStream::pair()?;
    let mut ime = reporter(client);
    let mut model = model();
    let service = thread::spawn(move || {
        let mut peer = BufReader::new(server);
        for (generation, (op, state, scope)) in [
            ("activate", Some("command"), "inactive"),
            ("activate", Some("command"), "applied"),
            ("state", Some("text"), "applied"),
            ("blur", None, "pending"),
            ("activate", Some("text"), "applied"),
            ("state", Some("command"), "applied"),
            ("resume", Some("command"), "applied"),
            ("close", None, "applied"),
        ]
        .into_iter()
        .enumerate()
        {
            receive(&mut peer, op, state);
            acknowledge(&mut peer, generation as u64 + 1, scope);
        }
    });
    let key = Event::Key(KeyEvent::new(KeyCode::Char('/'), KeyModifiers::NONE));
    assert!(!super::prepare_event(&mut ime, &key, &model));
    assert!(super::prepare_event(&mut ime, &key, &model));
    model = crate::app::update(model, crate::app::Message::StartSearch);
    ime.sync(super::text_mode(&model));
    assert!(!super::prepare_event(&mut ime, &Event::FocusLost, &model));
    model = crate::app::update(model, crate::app::Message::ClearSearch);
    ime.sync(super::text_mode(&model));
    assert!(super::prepare_event(
        &mut ime,
        &Event::Resize(90, 30),
        &model
    ));
    model = crate::app::update(model, crate::app::Message::StartSearch);
    assert!(!super::prepare_event(&mut ime, &Event::FocusGained, &model));
    model = crate::app::update(model, crate::app::Message::ClearSearch);
    ime.sync(super::text_mode(&model));
    assert!(super::prepare_event(&mut ime, &key, &model));
    ime.quit();
    service.join().unwrap();
    Ok(())
}

#[test]
fn external_editor_success_and_spawn_failure_suspend_before_handoff() -> Result<()> {
    for command in ["/usr/bin/true", "/nonexistent/markless-ime-editor"] {
        let (client, server) = UnixStream::pair()?;
        let mut ime = reporter(client);
        let service = thread::spawn(move || {
            let mut peer = BufReader::new(server);
            for (generation, (op, state)) in [
                ("activate", Some("command")),
                ("suspend", None),
                ("activate", Some("command")),
                ("close", None),
            ]
            .into_iter()
            .enumerate()
            {
                receive(&mut peer, op, state);
                acknowledge(&mut peer, generation as u64 + 1, "applied");
            }
        });
        let mut model = model();
        model.external_editor = Some(command.into());
        model.mouse_enabled = false;
        ime.focus_in(false);
        crate::app::App::handle_message_side_effects_with_ime(
            &mut model,
            &mut None,
            &crate::app::Message::EnterEditMode,
            Some(&mut ime),
        );
        assert!(model.needs_full_redraw);
        assert!(!model.editor_mode);
        assert!(
            ime.command_ready(),
            "terminal return is acknowledged before input"
        );
        ime.sync(false);
        assert!(ime.command_ready());
        ime.quit();
        service.join().unwrap();
    }
    Ok(())
}

#[test]
fn dropping_reporter_releases_lease_by_eof() -> Result<()> {
    let (client, server) = UnixStream::pair()?;
    let mut ime = reporter(client);
    let service = thread::spawn(move || {
        let mut peer = BufReader::new(server);
        receive(&mut peer, "activate", Some("command"));
        acknowledge(&mut peer, 1, "applied");
        peer.get_mut()
            .set_read_timeout(Some(Duration::from_secs(1)))
            .unwrap();
        let mut frame = String::new();
        assert_eq!(peer.read_line(&mut frame).unwrap(), 0);
    });
    ime.focus_in(false);
    drop(ime);
    service.join().unwrap();
    Ok(())
}

#[test]
fn explicit_blur_blocks_input_and_child_return_until_focus_in() -> Result<()> {
    for blur_first in [true, false] {
        let (client, server) = UnixStream::pair()?;
        let mut ime = reporter(client);
        let service = thread::spawn(move || {
            let mut peer = BufReader::new(server);
            let lifecycle = if blur_first {
                ["blur", "suspend"]
            } else {
                ["suspend", "blur"]
            };
            receive(&mut peer, "activate", Some("command"));
            acknowledge(&mut peer, 1, "applied");
            for op in lifecycle {
                receive(&mut peer, op, None);
                acknowledge(&mut peer, 2, "applied");
            }
            receive(&mut peer, "activate", Some("command"));
            acknowledge(&mut peer, 3, "applied");
            receive(&mut peer, "close", None);
            acknowledge(&mut peer, 4, "applied");
        });
        ime.focus_in(false);
        if blur_first {
            ime.focus_out();
            ime.input(false);
            ime.input(true);
            ime.stop();
        } else {
            ime.stop();
            ime.focus_out();
        }
        ime.resume(false);
        ime.input(false);
        ime.sync(true);
        assert!(!ime.focused);
        assert!(!ime.command_ready());
        ime.focus_in(false);
        assert!(ime.command_ready());
        ime.quit();
        service.join().unwrap();
    }
    Ok(())
}
