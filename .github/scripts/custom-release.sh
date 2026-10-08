#!/usr/bin/env bash
set -euo pipefail
[[ $# == 5 && $1 == build ]]
platform=$2 tag=$3 sha=$4 out=$5
[[ $(git rev-parse HEAD) == "$sha" && $out == /* && ! -e "$out" ]]
case "$platform:$(uname -s):$(uname -m)" in
 darwin:Darwin:arm64) triple=aarch64-apple-darwin ;;
 linux:Linux:aarch64) triple=aarch64-unknown-linux-gnu ;;
 *) echo 'native ARM64 runner required' >&2; exit 1 ;;
esac
rustup toolchain install 1.95.0 --profile minimal --target "$triple"
export CARGO_TARGET_DIR="$RUNNER_TEMP/markless-target"
cargo +1.95.0 build --locked --release --target "$triple"
mkdir -p "$out/bin"
cp "$CARGO_TARGET_DIR/$triple/release/markless" "$out/bin/"
[[ $("$out/bin/markless" --version) == "markless ${tag#v}" ]]
python3 - "$out/bin/markless" "$platform" <<'PYARCH'
import pathlib, struct, sys
head = pathlib.Path(sys.argv[1]).read_bytes()[:64]
if sys.argv[2] == 'linux':
    assert head[:4] == b'\x7fELF' and struct.unpack_from('<H',head,18)[0] == 183
else:
    assert head[:4] == b'\xcf\xfa\xed\xfe' and struct.unpack_from('<I',head,4)[0] == 0x100000c
PYARCH
