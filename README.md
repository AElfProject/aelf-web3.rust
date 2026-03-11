# aelf-sdk.rust

Async Rust SDK for [AElf](https://github.com/AElfProject) with a workspace layout that separates client, crypto, keystore, proto, contract wrappers, and the public facade crate.

## Overview

`aelf-sdk.rust` targets the same core client workflows covered by the official C#, Go, and Python SDKs, and adds the wallet, JS-compatible keystore, and `contract_at` dynamic contract flow that developers commonly use from `aelf-web3.js`.

Current v0.1 alpha scope:

- Async-first HTTP client for block, chain, network, transaction, and utility endpoints
- Wallet creation from random entropy, private key, or mnemonic
- JS-compatible keystore import/export with `aes-128-ctr + scrypt`
- Typed wrappers for `BasicContractZero`, `Token`, `AEDPoS`, `CrossChain`, `Election`, and `Vote`
- Dynamic contracts backed by `prost-reflect` and on-chain descriptor loading
- Vendored proto pipeline with `prost-build + pbjson-build`

Out of scope for v0.1:

- Browser `wasm32-unknown-unknown` runtime support
- Rust-only keystore format
- Business toolkits similar to `toolkits.py`

## Feature Matrix

| Area | Status | Notes |
| --- | --- | --- |
| Chain / block / net / tx client | Implemented | Async services in `aelf-client` |
| Wallet / mnemonic / transaction signing | Implemented | BIP44 path defaults to `m/44'/1616'/0'/0/0` |
| JS-compatible keystore | Implemented | Compatible with `dklen` and `dkLen` |
| Typed system contracts | Implemented | Zero, Token, Election, Vote, CrossChain, AEDPoS |
| Dynamic contract calls | Implemented | `call_typed`, `call_json`, `send_typed`, `send_json` |
| Proto vendoring pipeline | Implemented | `scripts/sync_proto.sh` |
| Local node integration test scaffold | Implemented | Ignored by default, opt in with `-- --ignored` |
| `wasm32-wasip2` custom-provider support | Implemented | Use `default-features = false` + `AElfClient::with_provider(...)` |
| Browser `wasm32-unknown-unknown` support | Planned | Post-v1 |

## Install

Use crates.io for published releases, or a path dependency while working against the workspace tip.

The latest published release on crates.io is currently `0.1.0-alpha.0`. The `0.1.0-alpha.1` workspace tip includes the new `wasm32-wasip2` custom-provider flow and can be consumed via a path dependency until it is published.

```toml
[dependencies]
aelf-sdk = "0.1.0-alpha.0"
tokio = { version = "1", features = ["macros", "rt"] }
```

Path dependency:

```toml
[dependencies]
aelf-sdk = { path = "crates/aelf-sdk" }
tokio = { version = "1", features = ["macros", "rt"] }
```

## Quick Start

```rust
use aelf_sdk::{AElfClient, ClientConfig};

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let client = AElfClient::new(ClientConfig::new("http://127.0.0.1:8000"))?;
    let status = client.chain().get_chain_status().await?;

    println!(
        "chain_id={} best_height={} genesis={}",
        status.chain_id,
        status.best_chain_height,
        status.genesis_contract_address
    );

    Ok(())
}
```

## Wallet & Keystore

```rust
use aelf_sdk::{Keystore, Wallet};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let wallet = Wallet::create()?;
    let keystore = Keystore::encrypt_js(&wallet, "123123")?;
    let unlocked = keystore.unlock_js("123123")?;

    assert_eq!(wallet.address(), unlocked.address);
    assert_eq!(wallet.private_key(), unlocked.private_key);
    assert_eq!(wallet.mnemonic(), unlocked.mnemonic);
    Ok(())
}
```

## Raw Transaction

The sample private key below is a public test-only readonly key. Never fund it.

```rust
use aelf_sdk::proto::token::TransferInput;
use aelf_sdk::{AElfClient, ClientConfig, Wallet, decode_address};
use prost::Message;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let client = AElfClient::new(ClientConfig::new("http://127.0.0.1:8000"))?;
    // Public test-only readonly key. Never fund it.
    let wallet = Wallet::from_private_key(
        "0000000000000000000000000000000000000000000000000000000000000001",
    )?;

    let tx = client
        .transaction_builder()
        .with_wallet(wallet)
        .with_contract("TOKEN_CONTRACT_ADDRESS")
        .with_method("Transfer")
        .with_message(&TransferInput {
            to: Some(aelf_sdk::proto::aelf::Address {
                value: decode_address("ELF_2J...")?,
            }),
            symbol: "ELF".to_owned(),
            amount: 1_0000_0000,
            memo: "transfer from rust sdk".to_owned(),
        })
        .build_signed()
        .await?;

    let raw_transaction = hex::encode(tx.encode_to_vec());
    let result = client.tx().send_transaction(&raw_transaction).await?;
    println!("{}", result.transaction_id);
    Ok(())
}
```

Public-node note:

- `send_transaction` accepts signed protobuf bytes encoded as hex.
- `/api/blockChain/rawTransaction` expects `Params` as protobuf JSON, not hex-encoded protobuf bytes.
- `execute_raw_transaction` and `send_raw_transaction` must sign the raw transaction bytes returned by the node.

## Typed Contracts

The sample private key below is a public test-only readonly key. Never fund it.

```rust
use aelf_sdk::proto::token::GetBalanceInput;
use aelf_sdk::{AElfClient, ClientConfig, Wallet, address_to_pb};

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let client = AElfClient::new(ClientConfig::new("http://127.0.0.1:8000"))?;
    // Public test-only readonly key. Never fund it.
    let wallet = Wallet::from_private_key(
        "0000000000000000000000000000000000000000000000000000000000000001",
    )?;
    let token = client.token_contract("TOKEN_CONTRACT_ADDRESS", wallet);

    let balance = token
        .get_balance(&GetBalanceInput {
            symbol: "ELF".to_owned(),
            owner: Some(address_to_pb("ELF_2J...")?),
        })
        .await?;

    println!("{}", balance.balance);
    Ok(())
}
```

## Dynamic Contracts

The sample private key below is a public test-only readonly key. Never fund it.

```rust
use aelf_sdk::{AElfClient, ClientConfig, Wallet};
use serde_json::json;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let client = AElfClient::new(ClientConfig::new("http://127.0.0.1:8000"))?;
    // Public test-only readonly key. Never fund it.
    let wallet = Wallet::from_private_key(
        "0000000000000000000000000000000000000000000000000000000000000001",
    )?;

    let contract = client.contract_at("TOKEN_CONTRACT_ADDRESS", wallet).await?;
    let balance = contract
        .call_json(
            "GetBalance",
            json!({
                "symbol": "ELF",
                "owner": "ELF_2J..."
            }),
        )
        .await?;

    println!("{balance}");
    Ok(())
}
```

## Examples

The canonical example sources live in `crates/aelf-sdk/examples`. The root `/examples` directory now contains thin forwarding wrappers so there is only one maintained implementation.

```bash
cargo run -p aelf-sdk --example basic_client
cargo run -p aelf-sdk --example wallet_keystore_roundtrip
cargo run -p aelf-sdk --example public_balance
cargo run -p aelf-sdk --example token_transfer
cargo run -p aelf-sdk --example dynamic_contract_get_balance
cargo run -p aelf-sdk --example raw_transaction_flow
```

Useful environment variables:

- `AELF_ENDPOINT`
- `AELF_PRIVATE_KEY`
- `AELF_TOKEN_CONTRACT`
- `AELF_TO_ADDRESS`
- `AELF_OWNER_ADDRESS`
- `AELF_AMOUNT`
- `AELF_SEND`

`public_balance` and `dynamic_contract_get_balance` fall back to a public test-only readonly key when `AELF_PRIVATE_KEY` is omitted. Never fund it.

## Feature Flags

v0.1 alpha exposes one transport feature:

- `native-http` (default): enables `HttpProvider` and `AElfClient::new(...)`
- `default-features = false`: keeps the SDK transport-agnostic so a host runtime can provide its own `Provider`

Transport-independent builds still support wallet, keystore, transaction building, typed contracts, and dynamic contracts through `AElfClient::with_provider(...)`.

## Native WASM (`wasm32-wasip2`)

`aelf-sdk` can now be consumed from native-wasm skill runtimes such as Portkey-style `wasm32-wasip2` sidecars.

Use the facade without native HTTP:

```toml
[dependencies]
aelf-sdk = { version = "0.1.0-alpha.1", default-features = false }
async-trait = "0.1"
http = "1"
serde_json = "1"
```

Then implement `Provider` in the host runtime and build the client with `with_provider(...)`:

```rust
use aelf_sdk::{AElfClient, AElfError, Provider};
use async_trait::async_trait;
use http::Method;
use serde_json::Value;

#[derive(Clone)]
struct HostProvider;

#[async_trait]
impl Provider for HostProvider {
    async fn request_json(
        &self,
        _method: Method,
        _path: &str,
        _query: &[(&str, String)],
        _body: Option<Value>,
    ) -> Result<Value, AElfError> {
        Err(AElfError::request("host transport not wired", None))
    }

    async fn request_text(
        &self,
        _method: Method,
        _path: &str,
        _query: &[(&str, String)],
        _body: Option<Value>,
    ) -> Result<String, AElfError> {
        Err(AElfError::request("host transport not wired", None))
    }
}

let client = AElfClient::with_provider(HostProvider)?;
# let _ = client;
```

Notes:

- The SDK does not own Portkey / IronClaw `walletExport` or workspace-memory contracts.
- Host runtimes should keep their own HTTP bindings and wallet storage layers, and delegate chain protocol logic to `aelf-sdk`.
- Browser `wasm32-unknown-unknown` remains out of scope for this alpha line.

## Transport Behavior

`HttpProvider` retries transient failures by default:

- `RetryPolicy::default()` = `2` retries with `200ms` initial backoff
- Retries apply to `5xx` responses and transport-level temporary errors
- `4xx` responses are returned immediately without retry

You can customize or disable retries:

```rust
use aelf_sdk::{AElfClient, ClientConfig, RetryPolicy};
use std::time::Duration;

let client = AElfClient::new(
    ClientConfig::new("https://aelf-public-node.aelf.io")
        .with_retry_policy(RetryPolicy::new(4, Duration::from_millis(100))),
)?;

let no_retry = AElfClient::new(
    ClientConfig::new("https://aelf-public-node.aelf.io")
        .without_retries(),
)?;
# let _ = (client, no_retry);
```

`send_transaction` only treats DTO payloads or transaction-id-shaped strings as success. Non-empty text such as `"ok"` or proxy error bodies are rejected as unexpected responses.

`client.contract_at(...)` still fetches a fresh descriptor whenever you create a new dynamic handle. Typed contract wrappers now lazily cache the first descriptor per handle instance and reuse it across subsequent calls and clones, without reintroducing a process-wide ABI cache.

## Local Node Testing

Compile everything:

```bash
cargo check --workspace
cargo check --workspace --examples
cargo check -p aelf-client --target wasm32-wasip2 --no-default-features
cargo check -p aelf-contract --target wasm32-wasip2 --no-default-features
cargo check -p aelf-sdk --target wasm32-wasip2 --no-default-features
```

Run unit tests:

```bash
cargo test --workspace
```

The default workspace test pass intentionally excludes the ignored live public-node smoke suite so local and CI runs remain deterministic.

Run the public readonly smoke suite:

```bash
cargo test -p aelf-sdk --test public_readonly_smoke -- --ignored --test-threads=1 --nocapture
```

Run ignored local-node integration tests:

```bash
cargo test -p aelf-sdk --test local_node -- --ignored
```

The ignored suite expects:

- `AELF_ENDPOINT`
- `AELF_PRIVATE_KEY`
- `AELF_TOKEN_CONTRACT`
- `AELF_TO_ADDRESS`

Manual funded transaction smoke is available through `.github/workflows/transaction-smoke.yml` and expects these repository secrets:

- `AELF_TRANSACTION_SMOKE_ENDPOINT`
- `AELF_TRANSACTION_SMOKE_PRIVATE_KEY`
- `AELF_TRANSACTION_SMOKE_TO_ADDRESS`
- `AELF_TRANSACTION_SMOKE_TOKEN_CONTRACT` (optional)
- `AELF_TRANSACTION_SMOKE_AMOUNT` (optional)

## Public Node Verification

Verified on March 10, 2026 against:

- Main chain: [https://aelf-public-node.aelf.io/swagger/index.html](https://aelf-public-node.aelf.io/swagger/index.html)
- Side chain: [https://tdvv-public-node.aelf.io/swagger/index.html](https://tdvv-public-node.aelf.io/swagger/index.html)

Validated flows:

- `basic_client` against both public nodes
- readonly block / chain / net / tx pool integration tests against both public nodes
- Zero -> Token typed contract calls against both public nodes
- `contract_at(...).call_json("GetBalance", ...)` against both public nodes
- `send_transaction` against main chain
- `create_raw_transaction -> execute_raw_transaction -> send_raw_transaction -> get_transaction_result` against main chain

Observed compatibility notes:

- Dynamic JSON input/output normalizes `aelf.Address` and `aelf.Hash` between developer-friendly strings and protobuf JSON objects.
- `create_raw_transaction` succeeds when `Params` is protobuf JSON. Passing hex-encoded protobuf bytes returns `403 Invalid params` on the public main-chain node.
- The signature used by `execute_raw_transaction` and `send_raw_transaction` must be computed from the node-produced raw transaction bytes.
- `calculate_transaction_fee` may return an empty fee map on public nodes even when the transaction is accepted and mined.

Useful commands:

```bash
AELF_ENDPOINT='https://aelf-public-node.aelf.io' cargo run -p aelf-sdk --example basic_client
AELF_ENDPOINT='https://aelf-public-node.aelf.io' AELF_OWNER_ADDRESS='<address>' cargo run -p aelf-sdk --example public_balance
AELF_ENDPOINT='https://aelf-public-node.aelf.io' AELF_PRIVATE_KEY='<private-key>' AELF_TO_ADDRESS='ELF_<address>_AELF' AELF_AMOUNT='1' cargo run -p aelf-sdk --example token_transfer
AELF_ENDPOINT='https://aelf-public-node.aelf.io' AELF_PRIVATE_KEY='<private-key>' AELF_TO_ADDRESS='ELF_<address>_AELF' AELF_AMOUNT='1' cargo run -p aelf-sdk --example raw_transaction_flow
```

## Release

Workspace layout:

```text
crates/
  aelf-sdk
  aelf-client
  aelf-contract
  aelf-crypto
  aelf-keystore
  aelf-proto
examples/
proto/upstream/
scripts/sync_proto.sh
tests/fixtures/
```

Release flow:

1. Sync upstream proto files with `scripts/sync_proto.sh`
2. Run `cargo fmt`, `cargo +1.85.0 check --workspace --all-targets --all-features --locked`, `cargo clippy --workspace --all-targets --all-features`, `cargo audit`, `cargo check --workspace --examples`, `cargo test --workspace`
3. Review `CHANGELOG.md`, then run the manual `publish` GitHub Actions workflow with `dry_run=true`. Leave `packages` empty for a full release dry-run, or set `packages=aelf-sdk` to verify a targeted recovery path.
4. Re-run the `publish` workflow with `dry_run=false` after confirming the crates.io token is configured. Leave `skip_published=true` so retries can safely resume after a partial publish.
5. If crates.io returns a transient error after some crates are already published, rerun the workflow with `dry_run=false`, `packages=<remaining-crates>`, and `skip_published=true`. For the March 10, 2026 incident, the recovery command is `packages=aelf-sdk`.

Publishing notes:

- The publish workflow releases crates in dependency order: `aelf-proto`, `aelf-crypto`, `aelf-client`, `aelf-keystore`, `aelf-contract`, `aelf-sdk`.
- Full-workspace dry-runs still use `cargo publish --workspace --dry-run --locked` so unpublished interdependent versions can be validated together.
- crates.io releases are immutable. If a published version is wrong, it must be `yank`ed and replaced with a new version.

CI is defined in `.github/workflows/ci.yml`.
Publishing is defined in `.github/workflows/publish.yml` and expects the `CARGO_REGISTRY_TOKEN` repository secret.

MSRV:

- The workspace MSRV is Rust `1.85`.
- CI enforces it with `cargo +1.85.0 check --workspace --all-targets --all-features --locked`.

## Security

See [SECURITY.md](SECURITY.md) for private vulnerability disclosure instructions.

## License

MIT
