# Contributing

## Development Setup

1. Install Rust `1.85` or newer.
2. Clone the repository and fetch submodules if your workflow requires them.
3. Build the workspace:

```bash
cargo check --workspace
```

## Required Checks

Run these commands before opening a PR:

```bash
cargo fmt --all
cargo +1.85.0 check --workspace --all-targets --all-features --locked
cargo clippy --workspace --all-targets --all-features -- -D warnings
cargo audit
cargo check --workspace --examples
cargo test --workspace
```

Optional local-node validation:

```bash
cargo test -p aelf-sdk --test local_node -- --ignored
```

## Repository Layout

- `crates/aelf-sdk`: public facade crate
- `crates/aelf-client`: HTTP client, DTOs, transaction builder
- `crates/aelf-contract`: typed and dynamic contract bindings
- `crates/aelf-crypto`: wallet, signing, address utilities
- `crates/aelf-keystore`: JS-compatible keystore support
- `crates/aelf-proto`: generated protobuf bindings
- `examples/`: runnable examples wired into `aelf-sdk`

## Pull Requests

- Keep changes scoped to one problem or feature.
- Add or update tests for every behavior change.
- Document public API additions with rustdoc.
- Update `README.md`, `README.zh.md`, or `CHANGELOG.md` when user-facing behavior changes.
- Keep the documented MSRV at Rust `1.85` and preserve the hard `cargo +1.85.0 check --workspace --all-targets --all-features --locked` CI gate.

## Commit Style

Use Conventional Commit / `git-cz` style messages in English and keep the matching emoji prefix used by the repository workflow.
