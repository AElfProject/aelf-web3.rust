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

- WASM/browser runtime support
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
| WASM support | Planned | Post-v1 |

## Install

Until the crate is published to crates.io, use a path or git dependency.

```toml
[dependencies]
aelf-sdk = { path = "crates/aelf-sdk" }
tokio = { version = "1", features = ["macros", "rt-multi-thread"] }
```

Git dependency:

```toml
[dependencies]
aelf-sdk = { git = "https://github.com/AElfProject/aelf-web3.rust", tag = "v0.1.0-alpha.0" }
tokio = { version = "1", features = ["macros", "rt-multi-thread"] }
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

```rust
use aelf_sdk::proto::token::TransferInput;
use aelf_sdk::{AElfClient, ClientConfig, Wallet, decode_address};
use prost::Message;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let client = AElfClient::new(ClientConfig::new("http://127.0.0.1:8000"))?;
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

```rust
use aelf_sdk::proto::token::GetBalanceInput;
use aelf_sdk::{AElfClient, ClientConfig, Wallet, address_to_pb};

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let client = AElfClient::new(ClientConfig::new("http://127.0.0.1:8000"))?;
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

```rust
use aelf_sdk::{AElfClient, ClientConfig, Wallet};
use serde_json::json;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let client = AElfClient::new(ClientConfig::new("http://127.0.0.1:8000"))?;
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

Examples are stored in `/examples` and wired into the `aelf-sdk` crate.

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

## Feature Flags

v0.1 alpha intentionally keeps the surface small. There are no optional Cargo feature flags yet.

- Runtime: Tokio only
- TLS: `reqwest` with `rustls`
- Proto JSON: `pbjson`

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

Dynamic contract descriptors are cached in-memory using a `64`-entry LRU cache to avoid unbounded growth in long-running services.

## Local Node Testing

Compile everything:

```bash
cargo check --workspace
cargo check --workspace --examples
```

Run unit tests:

```bash
cargo test --workspace
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

## License

MIT
