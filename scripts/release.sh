#!/usr/bin/env bash
# Build and check the release folder ../yafm-release/yafm-VERSION/:
# static + glibc binaries, the source (unpacked and as a tarball), a vendored
# source tarball for offline builds, README.md, INSTALL.md, COPYING, SHA256SUMS.
set -euo pipefail
cd "$(dirname "$0")/.."

if [ -n "$(git status --porcelain)" ]; then
    echo "release: the working tree has uncommitted changes" >&2
    exit 1
fi
version="$(sed -n 's/^version = "\(.*\)"/\1/p' Cargo.toml | head -1)"
name="yafm-$version"
out="$(cd .. && pwd)/yafm-release/$name"

echo "== tests"
cargo test -q 2>&1 | grep -E "^test result|FAILED|panicked" || true
cargo test -q >/dev/null 2>&1 || { echo "release: tests fail" >&2; exit 1; }
echo "== minimum Rust version covers every locked dependency"
python3 - <<'PY'
import json, re, subprocess, sys
own = re.search(r'^rust-version = "([0-9.]+)"', open("Cargo.toml").read(), re.M).group(1)
meta = json.loads(subprocess.check_output(["cargo", "metadata", "--format-version", "1", "--locked",
                                           "--filter-platform", "x86_64-unknown-linux-gnu"]))
used = {n["id"] for n in meta["resolve"]["nodes"]}
key = lambda v: tuple((list(map(int, v.split("."))) + [0, 0])[:3])
need = max(((p.get("rust_version") or "0", p["name"]) for p in meta["packages"] if p["id"] in used),
           key=lambda v: key(v[0]))
if key(need[0]) > key(own):
    sys.exit(f"release: {need[1]} needs Rust {need[0]} but Cargo.toml says {own}")
print(f"Cargo.toml rust-version {own} >= {need[0]} ({need[1]})")
PY
echo "== builds"
cargo build -q --release --target x86_64-unknown-linux-gnu
cargo build -q --release --target x86_64-unknown-linux-musl
musl="target/x86_64-unknown-linux-musl/release/yafm"
echo "== smoke test (static binary in a pseudo-terminal)"
python3 -I tests/pty_smoke.py "$musl"

echo "== package $out"
rm -rf "$out"
mkdir -p "$out/src"
cp "$musl" "$out/$name-x86_64-linux-musl"
cp target/x86_64-unknown-linux-gnu/release/yafm "$out/$name-x86_64-linux-gnu"
git archive --format=tar --prefix="$name/" HEAD | tar -x -C "$out/src"
tar -C "$out/src" -czf "$out/$name-src.tar.gz" "$name"

vend="$(mktemp -d)"
check="$(mktemp -d)"
trap 'rm -rf "$vend" "$check"' EXIT
git archive --format=tar --prefix="$name/" HEAD | tar -x -C "$vend"
(cd "$vend/$name" && mkdir -p .cargo && cargo vendor --locked vendor 2>/dev/null > .cargo/config.toml)
if ! grep -q 'replace-with = "vendored-sources"' "$vend/$name/.cargo/config.toml"; then
    echo "release: cargo vendor wrote no source replacement" >&2
    exit 1
fi
# Windows-only crates are never built on Linux, but Cargo still reads their
# manifests: keep Cargo.toml, empty the rest (about 70% of the vendor folder).
python3 -I - "$vend/$name/vendor" <<'PY'
import glob, json, os, shutil, sys
stubs = sorted(glob.glob(sys.argv[1] + "/windows*") + glob.glob(sys.argv[1] + "/winapi*"))
for d in stubs:
    for e in os.listdir(d):
        p = os.path.join(d, e)
        if e not in ("Cargo.toml", ".cargo-checksum.json"):
            shutil.rmtree(p) if os.path.isdir(p) else os.remove(p)
    os.makedirs(d + "/src")
    with open(d + "/src/lib.rs", "w") as f:
        f.write("// stub: Windows-only crate, not built on Linux\n")
    ck = json.load(open(d + "/.cargo-checksum.json"))
    ck["files"] = {}
    json.dump(ck, open(d + "/.cargo-checksum.json", "w"))
print(f"stubbed {len(stubs)} Windows-only crates")
PY
tar -C "$vend" -czf "$out/$name-src-vendored.tar.gz" "$name"

cp README.md INSTALL.md COPYING "$out/"
# Flat names, so SHA256SUMS also checks files downloaded side by side (GitHub assets).
(cd "$out" && sha256sum "$name"-x86_64-* "$name"-*.tar.gz > SHA256SUMS)

echo "== checks"
file "$out/$name-x86_64-linux-musl" | grep -Eq "static(-pie)? linked"
"$out/$name-x86_64-linux-musl" --version
(cd "$out" && sha256sum -c --quiet SHA256SUMS)
tar -xzf "$out/$name-src-vendored.tar.gz" -C "$check"
# An empty CARGO_HOME behaves like a fresh machine: nothing cached, no network.
mkdir -p "$check/cargo-home"
(cd "$check/$name" && CARGO_HOME="$check/cargo-home" CARGO_TARGET_DIR="$check/target" cargo build -q --release --offline --frozen)
"$check/target/release/yafm" --version
echo "== done: $out"
