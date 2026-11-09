# hapsigner (vendored)

Source: https://github.com/harmony-contrib/hapsigner-rs
Snapshot: `main` downloaded 2026-10-14 (zip comment `fcec93743146f72fadc05f253bbc6251ffc6bc26`).
License: MIT; see `LICENSE` and `THIRD_PARTY_NOTICES.md` in this directory.

Local changes:

- `Cargo.toml`: relaxed `time` from an exact `=0.3.44` pin to `0.3.44`, so the
  workspace can continue using `time 0.3.55` required by GPUI. The signing code
  does not depend on a specific patch release.
- Removed upstream `tests/` and build metadata; `src/` is otherwise unchanged.

The crate is consumed with `default-features = false`, so the development CLI
and the embedded OpenHarmony test identity are not compiled into RareBox.
