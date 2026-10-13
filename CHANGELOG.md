# Changelog

## v0.6.9 (2026-10-13)

### Fixes

- update `repository` URL in `Cargo.toml`
- update `Cargo.toml` `authors` field (add Karolin Varner)
- fix license text files and `README.md` to reflect dual license (Apache 2 / MIT)

## v0.6.8 (2026-10-13)

### Fixes

- set MSRV to `1.88.0` (previously no MSRV set)
- remove deprecated dependency `serde_yaml` and switch to `yaml_serde` (resolves [RUSTSEC advisory](https://rustsec.org/advisories/RUSTSEC-2024-0449))
- upgrade dependencies
- fix `clippy` warnings

### Other

- add badges to `README.md`
- set up Dependabot
- set up CI
  - automatic builds for different channels (`stable`, `beta`, `nightly`)
  - automatic tests for different channels (`stable`, `beta`, `nightly`), including feature combinations (`cargo-test-feature-combinations.sh`)
  - check that build and test work with updated dependencies instead of the ones locked in `Cargo.lock`
  - verify that specified Rust version is correct
  - check for `clippy` warnings

## v0.6.7 (2026-06-28)

### Features

#### make `zstd-offload` an optional feature

The `zstd-offlead` feature can be enabled via `cargo` and allows users to compress offloaded values with `zstd` before writing their sidecar files. When disabled, offloaded values are written uncompressed. Previously, `zstd` was always on and could not be turned off, now the user can control this behavior.

Additionally, the file extension for offloaded values is changed to:

- `.zstd` if feature is enabled
- `.bin` if feature is disabled

Previously, the file extension for offloaded values was always `.zstd`.

### Fixes

- fix a test that deadlocks if `tls` feature is disabled
- add a test script which tests all feature combinations (`cargo-test-feature-combinations.sh`)
- clean up `Cargo.toml` workspace and deduplicate depenency declarations
- upgrade dependencies

## Older Versions

Older versions are not yet listet in the changelog. There is an open issue on GitHub: <https://github.com/rosenpass/assert_tv/issues/23>
