#!/usr/bin/env bash
# Runs the pinned map and chunk hash tests for aarch64 under qemu-user, with no sudo: zig links, qemu-user-static is unpacked from a .deb.
# usage: scripts/arm64-pins.sh [tool-dir]   (default /tmp/ochre-arm64; needs python3, apt-get, rustup, cargo)
set -euo pipefail
T=${1:-/tmp/ochre-arm64}; TARGET=aarch64-unknown-linux-musl
mkdir -p "$T/pc"; cd "$(dirname "$0")/.."
[ -x "$T/q/usr/bin/qemu-aarch64-static" ] || { (cd "$T" && apt-get download qemu-user-static && dpkg -x qemu-user-static*.deb q); }
[ -x "$T/zv/bin/python" ] || { python3 -m venv "$T/zv" && "$T/zv/bin/pip" install -q ziglang; }
rustup target add $TARGET
# bevy's sys crates need pkg-config answers to compile; the pin tests never call them
for p in alsa libudev; do printf 'Name: %s\nDescription: stub\nVersion: 999\nLibs:\nCflags:\n' $p > "$T/pc/$p.pc"; done
# zig supplies its own crt files and rejects the cortex-a53 erratum flag
cat > "$T/zcc.sh" <<EOS
#!/bin/sh
args=""
for a in "\$@"; do case "\$a" in --target=*|*/self-contained/crt*.o|-Wl,--fix-cortex-a53-843419) ;; *) args="\$args \$a";; esac; done
exec $T/zv/bin/python -m ziglang cc -target aarch64-linux-musl \$args
EOS
chmod +x "$T/zcc.sh"
export CARGO_TARGET_AARCH64_UNKNOWN_LINUX_MUSL_LINKER=$T/zcc.sh CC_aarch64_unknown_linux_musl=$T/zcc.sh
export PKG_CONFIG_ALLOW_CROSS=1 PKG_CONFIG_PATH=$T/pc PKG_CONFIG_LIBDIR=$T/pc
run() { # crate, test filter
  local exe
  exe=$(cargo test --release -j 5 --target $TARGET -p "$1" --lib --no-run --message-format=json 2>/dev/null |
    python3 -c 'import sys,json
for l in sys.stdin:
    m=json.loads(l)
    if m.get("executable") and m.get("profile",{}).get("test"): print(m["executable"])' | tail -n 1)
  "$T/q/usr/bin/qemu-aarch64-static" "$exe" "$2"
}
run ochre-world portable_maths
run ochre-terrain determinism
run ochre-base noise
