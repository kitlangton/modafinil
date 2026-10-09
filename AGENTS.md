# Modafinil

Small Rust CLI for macOS keep-awake. The crate is `modafinil-cli`; the executable is `modafinil`. It is unrelated to other apps named Modafinil.

## Structure

- `src/lib.rs`: pure policy, parsers and launchd plist rendering.
- `src/system.rs`: bounded system commands, assertions, configuration and services.
- `src/main.rs`: CLI parsing and platform guard.
- `tests/`: policy and command-line tests, with no system mutations.

## Commands

```sh
cargo fmt --check
cargo test --locked
cargo clippy --all-targets --locked -- -D warnings
cargo package --locked
```

## Safety

- Default to AC-only ordinary keep-awake. Closed-lid support is opt-in and AC-only in every mode.
- Never install/start services, change `pmset`, run sudo, or perform power/lid experiments without explicit operator approval. Unit tests and CI must not do these things.
- Treat unreadable power/configuration as a reason to release ordinary assertions. Privileged check failures must attempt sleep restoration and retry if that fails.
- Use fixed absolute system executable paths and argument arrays, never shells, PATH lookup or user-supplied executable configuration.
- The root daemon runs from a root-owned installed copy and uses fixed launchd arguments. It must not execute user scripts or read arbitrary commands from user configuration.
- Do not install passwordless sudo rules. Respect other sleep tools; do not silently stop or remove them.
- `pmset disablesleep` is persistent and system-wide. Do not promise perfect cleanup after force kill or OS failure. Preserve documentation of recovery and thermal risks.
- Read back system/service state after mutations. Transport success alone is not verification.
- Keep public tests and docs free of personal paths, credentials, account data and internal model identifiers.

## Releases

Validate tests, formatting and Clippy; inspect `cargo package --list` and the packaged crate; build from a clean consumer before publishing. Publish with `cargo publish --locked`, then verify the registry download and install. Publishing and pushing require the owner's explicit approval. Do not add AI attribution to commits.

Tagged releases (`v<crate-version>`) build/test native Apple Silicon and Intel binaries in `.github/workflows/release.yml`, then publish tarballs and checksums to GitHub Releases. Both builds must pass before publication. Update `kitlangton/homebrew-tap` with the actual archive SHA-256 values and verify a real Homebrew install and formula test before sharing installation instructions. Homebrew installation must not start services or enable privileged features automatically.
