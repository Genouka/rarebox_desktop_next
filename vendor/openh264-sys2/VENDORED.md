# Vendored: `openh264-sys2` 0.9.9

This directory is a copy of the `openh264-sys2` crate from crates.io, patched to
allow cross-compiling for OpenHarmony (`aarch64-unknown-linux-ohos` /
`x86_64-unknown-linux-ohos`).

## Source

- Crate: `openh264-sys2` 0.9.9 (`BSD-2-Clause`), part of
  <https://github.com/ralfbiedert/openh264-rs>
- Copied from the local cargo registry cache:
  `~/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/openh264-sys2-0.9.9`
- Registry checksum of the unmodified crate:
  `414f5bd53da84e126dc9449eb6f0003bb6994b2de63c48119b0c9b7a8b28086a`
- Bundled upstream C/C++ source: Cisco OpenH264, commit
  `1bca62311e447fd9c39fc93de2c4b96cd5ce971c` (see `upstream/LICENSE`,
  `upstream/VERSION`).
- Registry packaging residue (`.cargo-ok`, `.cargo_vcs_info.json`,
  `Cargo.toml.orig`, `Cargo.lock`) was deleted; the crate's built-in `tests/`
  and `upstream/` trees are kept verbatim.

## License

- `openh264-sys2` itself: BSD-2-Clause (as declared in `Cargo.toml`).
- `upstream/` (OpenH264): BSD-2-Clause, Copyright (c) 2013, Cisco Systems —
  full text in `upstream/LICENSE`.

Both are compatible with the workspace's AGPL-3.0-only licensing under the
usual BSD-2-Clause terms (retain the copyright notice and disclaimer).

## Changes vs. upstream 0.9.9

Only `build.rs` was modified. Nothing else in the crate was touched.

1. `enum TargetEnv` gained an `Ohos` variant (previous code panicked with
   `Unknown target env: ohos` in `Target::from_env`).
2. The `CARGO_CFG_TARGET_ENV` match arm now maps `"ohos"` to
   `TargetEnv::Ohos`.

That is the whole patch (2 hunks). No other change is needed to `build.rs` for
OHOS: `aarch64-unknown-linux-ohos` reports `target_os = "linux"`,
`target_family = "unix"`, so the existing Linux code path already applies
(NASM auto-disabled for aarch64, aarch64 NEON `.S` files compiled into the
static libraries, `-fPIC` via `cc::Build::pic(true)`).

The C++ runtime is linked as `c++_shared` instead of cc-rs' default `c++` for
OHOS; that is done from the environment (`CXXSTDLIB_aarch64_unknown_linux_ohos`
in `script/ohos_cross.sh`) rather than by patching `build.rs`, because the NDK
ships `libc++_shared.so` / `libc++_static.a` and has no `libc++.so` alias in
`sysroot/usr/lib/<triple>`.

## How to consume it

Add to the root `Cargo.toml` (the desktop build is unaffected — the patch only
replaces the registry copy for the whole graph):

```toml
[patch.crates-io]
openh264-sys2 = { path = "vendor/openh264-sys2" }
```

Equivalent for a one-off invocation, via `script/ohos_cross.sh`:

```bash
bash script/ohos_cross.sh cargo check --target aarch64-unknown-linux-ohos -p rarebox-core \
    --config 'patch.crates-io.openh264-sys2.path="vendor/openh264-sys2"'
```

`Cargo.lock` keeps recording the registry source for `openh264-sys2` 0.9.9;
the patch does not require a lockfile rewrite as long as the version matches.

### Toolchain requirements (handled by `script/ohos_cross.sh`)

- `CC`/`CXX`/`AR` must point at the NDK `llvm/bin/clang.exe`,
  `clang++.exe`, `llvm-ar.exe`. On Windows, do **not** use the extension-less
  `aarch64-unknown-linux-ohos-clang` shell wrapper — it is not a Win32
  executable. cc-rs appends `--target=<triple>` itself.
- `CFLAGS`/`CXXFLAGS` need `--sysroot=<ndk>/sysroot`.
- cc-rs splits `CFLAGS` on ASCII whitespace and does not strip quotes, so the
  SDK path must not contain spaces; the script resolves the 8.3 short path
  (`C:/PROGRA~1/Huawei/DEVECO~1/sdk/default/OPENHA~1/native`).
- `cargo check` needs no linker. A real link additionally needs
  `-L <ndk>/llvm/lib/<triple>` so that `-lc++_shared` resolves.

## Verified

```
bash script/ohos_cross.sh cargo check --target aarch64-unknown-linux-ohos -p rarebox-core \
    --config 'patch.crates-io.openh264-sys2.path="vendor/openh264-sys2"'
```
→ `Finished dev profile ... in 41.76s` (exit 0). The build script log
(`target/aarch64-unknown-linux-ohos/debug/build/openh264-sys2-*/output`)
records `Target { family: Unix, os: Linux, arch: Aarch64, env: Ohos }`,
the NDK clang flags, and `cargo:rustc-link-lib=c++_shared`; produced objects
are `ELF 64-bit LSB relocatable, ARM aarch64`.

```
cargo check -p rarebox-core
```
→ exit 0 (host build unchanged).

## Maintenance

When bumping `openh264-sys2`, re-copy the crate and re-apply the two-hunk
`build.rs` patch until the upstream crate accepts `target_env = "ohos"`
(or once the workspace no longer needs the `source` feature on OHOS).
