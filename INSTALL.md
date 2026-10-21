# Installing yafm

Three ways, from easiest to most portable. All of them give you one program,
`yafm`, and nothing else is installed.

| You have | Use |
|---|---|
| An x86_64 Linux machine (most PCs and servers) | **A. static binary** — nothing to build |
| Any Linux/Unix machine with internet | **B. build from source** |
| Any Linux/Unix machine without internet | **C. offline build** (vendored source) |

Check the architecture of the remote machine with `uname -m`
(`x86_64` → A works; `aarch64`, `armv7l`, … → use B or C).

## Requirements

**To run yafm:**
- Linux (other Unixes should work with B/C, untested)
- a terminal at least 66 columns wide, with `TERM` set (any modern terminal or SSH session)

**Optional programs** yafm calls for some features (install the ones you need):

| Feature | Programs |
|---|---|
| View / edit / shell (defaults, changeable in config.toml) | `less`, `vi`, `bash` |
| Archives as folders, compress / uncompress | `tar`, `gzip`, `bzip2`, `xz`, `lzip`, `lz4`, `zip` + `unzip`, `rar`, `7z` |
| Find / grep | `find`, `grep` |
| File info, filesystems info | `file`, `df` |
| Diff with backup | `diff` |
| Permissions / owner | `chmod`, `chown`, `chgrp`, `getent` |

On Debian/Ubuntu: `sudo apt install less file xz-utils bzip2 lzip lz4 zip unzip p7zip-full`
(most of the others are always present).

**To build (B and C only):** Rust 1.88 or newer (`rustc --version`), with
`cargo`. Distribution packages (`apt install rustc`) are often older — use rustup. Get it from <https://rustup.rs> (one command, no root needed):
`curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh`.
Building takes 1–3 minutes and about 1 GB of disk for the build cache.

## A. Static binary (x86_64 Linux)

The file `yafm-VERSION-x86_64-linux-musl` runs on any x86_64 Linux,
whatever its C library version.

```sh
# on your machine: copy it to the remote machine
scp yafm-0.1.3-x86_64-linux-musl user@remote:/tmp/yafm

# on the remote machine: install for all users …
sudo install -m 755 /tmp/yafm /usr/local/bin/yafm
# … or just for you (make sure ~/.local/bin is in your PATH)
install -D -m 755 /tmp/yafm ~/.local/bin/yafm

yafm --version
```

`yafm-VERSION-x86_64-linux-gnu` is the same program linked against the
system C library; it needs glibc 2.39 or newer (Ubuntu 24.04, Debian 13,
Fedora 40 or later; check with `ldd --version`), so prefer the `musl` one.

Check the downloads with `sha256sum -c --ignore-missing SHA256SUMS` (run it in
the folder holding the downloaded files and SHA256SUMS).

## B. Build from source (with internet)

```sh
tar xzf yafm-0.1.3-src.tar.gz
cd yafm-0.1.3
cargo build --release          # downloads the Rust libraries from crates.io
sudo install -m 755 target/release/yafm /usr/local/bin/yafm
# or: cargo install --path .   (installs to ~/.cargo/bin)
```

The folder `src/yafm-0.1.3/` in the release is the same source, unpacked.

## C. Offline build (no internet on the remote machine)

`yafm-0.1.3-src-vendored.tar.gz` contains the source plus every Rust library
it needs, so `cargo` never goes online. Rust itself must already be installed
(copy the rustup installer and toolchain over if needed). Windows-only libraries
are reduced to empty stubs to keep the tarball small, so it builds on Linux and
other Unix systems, not on Windows.

```sh
tar xzf yafm-0.1.3-src-vendored.tar.gz
cd yafm-0.1.3
cargo build --release --offline --frozen
sudo install -m 755 target/release/yafm /usr/local/bin/yafm
```

## After installing

```sh
yafm --check-config    # creates ~/.config/yafm/ and checks it
yafm                   # start in the current directory
```

If you used lfm, its settings, keys and theme in `~/.config/lfm/` are
imported on the first run (they are not changed).

**Quit and cd:** to make `q` leave your shell in yafm's last directory, add
the `yafm()` shell function from README.md ("Quit and cd") to `~/.bashrc`
or `~/.zshrc`, then open a new shell.

## Uninstall

```sh
sudo rm /usr/local/bin/yafm      # or ~/.local/bin/yafm, or: cargo uninstall yafm
rm -r ~/.config/yafm             # settings, keys, theme, history (optional)
```
