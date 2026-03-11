# Changelog

All notable changes to `aelf-sdk.rust` will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project follows [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [0.1.0-alpha.1] - 2026-03-11

### Added

- `native-http` Cargo feature on `aelf-client`, `aelf-contract`, and `aelf-sdk`, enabled by default for native consumers
- `AElfClient::with_provider(...)` and `Provider` re-exports on the facade crate for custom transports
- `wasm32-wasip2` CI compile gates for `aelf-client`, `aelf-contract`, and `aelf-sdk` with `--no-default-features`
- public readonly smoke coverage in a dedicated CI job plus a manual funded transaction smoke workflow

### Changed

- The workspace version moved to `0.1.0-alpha.1`
- Tokio now uses the wasm-compatible `rt` feature instead of `rt-multi-thread`
- `HttpProvider` and `AElfClient::new(...)` are now gated behind `native-http`, while the core SDK remains provider-first
- root `/examples` now forward to `crates/aelf-sdk/examples` so the SDK only maintains one example source of truth
- wallet keystore examples now redact private key and mnemonic output

### Fixed

- `aelf-client`, `aelf-contract`, and `aelf-sdk` now compile for `wasm32-wasip2` when consumed with `default-features = false`
- The facade crate now exposes the transport abstraction needed by native-wasm skill runtimes
- `send_transaction` no longer treats arbitrary non-empty text payloads as success
- typed contract wrappers now lazily reuse the first descriptor per handle, while direct `contract_at(...)` calls still fetch a fresh descriptor for each new handle
- `ChainStatusDto` now accepts both public-node branch map shapes, preventing readonly smoke failures behind mixed main-chain gateway backends

## [0.1.0-alpha.0] - 2026-03-10

### Added

- Workspace-based Rust SDK layout with `aelf-sdk`, `aelf-client`, `aelf-contract`, `aelf-crypto`, `aelf-keystore`, and `aelf-proto`
- Typed wrappers for Zero, Token, Election, Vote, CrossChain, and AEDPoS contracts
- Dynamic contract calls backed by on-chain descriptor loading
- JS-compatible keystore import/export with fixture compatibility
- Public-node examples and verification flows for readonly calls and raw transactions
- Provider mock support and offline network-layer regression tests
- A manual GitHub Actions publish workflow for crates.io dry-runs and releases

### Changed

- Sensitive key material now uses zeroization and redacted `Debug` output
- `HttpProvider` now retries transient `5xx` and transport failures with configurable exponential backoff
- Dynamic contract descriptor cache now uses a bounded 64-entry LRU cache
- Public DTOs and contract APIs now include rustdoc coverage
- The workspace now declares Rust `1.85` as its MSRV and CI enforces it with a dedicated toolchain job

### Fixed

- Public-node compatibility for `send_transaction`, `create_raw_transaction`, and dynamic JSON address/hash normalization
- Null-safe deserialization for transaction result payloads returned by public nodes
- JS / C# keystore compatibility edge cases, including `dkLen` alias handling
- `wiremock` is pinned to the latest pre-`let-chain` release so test-only dependencies do not inflate the SDK MSRV
