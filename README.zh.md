# aelf-sdk.rust

面向 [AElf](https://github.com/AElfProject) 的异步 Rust SDK。仓库采用 workspace 结构，将 client、crypto、keystore、proto、contract wrapper 和 facade crate 分层实现。

## Overview

`aelf-sdk.rust` 的目标是对齐官方 C#、Go、Python SDK 的 core client 能力，并补齐 `aelf-web3.js` 常用的 wallet、JS-compatible keystore、`contract_at` 动态合约调用能力。

当前 v0.1 alpha 范围：

- `aelf-client` 提供 block、chain、network、transaction、utils 的异步 HTTP 服务
- `aelf-crypto` 支持随机钱包、私钥导入、助记词导入、交易签名
- `aelf-keystore` 提供与 JS SDK 兼容的 keystore 导入导出
- `aelf-contract` 提供 Zero、Token、AEDPoS、CrossChain、Election、Vote 的 typed wrapper
- `contract_at` 基于 `prost-reflect` 和链上 descriptor 实现动态合约调用
- proto pipeline 基于 vendored proto + `prost-build + pbjson-build`

当前 v0.1 不包含：

- WASM / browser runtime 支持
- Rust-only keystore 格式
- 类似 Python `toolkits.py` 的业务工具箱

## Feature Matrix

| 能力 | 状态 | 说明 |
| --- | --- | --- |
| Chain / block / net / tx client | 已实现 | 位于 `aelf-client` |
| Wallet / mnemonic / transaction signing | 已实现 | 默认 BIP44 路径 `m/44'/1616'/0'/0/0` |
| JS-compatible keystore | 已实现 | 同时兼容 `dklen` / `dkLen` |
| Typed system contracts | 已实现 | Zero、Token、Election、Vote、CrossChain、AEDPoS |
| Dynamic contract calls | 已实现 | `call_typed`、`call_json`、`send_typed`、`send_json` |
| Proto vendoring pipeline | 已实现 | `scripts/sync_proto.sh` |
| 本地节点集成测试脚手架 | 已实现 | 默认 ignored，需要手动启用 |
| WASM 支持 | 规划中 | v1 之后处理 |

## Install

在 crate 发布到 crates.io 之前，建议先用 path 或 git 依赖。

```toml
[dependencies]
aelf-sdk = { path = "crates/aelf-sdk" }
tokio = { version = "1", features = ["macros", "rt-multi-thread"] }
```

Git 依赖：

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

公网节点补充说明：

- `send_transaction` 接收的是“已签名 protobuf bytes 的 hex 字符串”。
- `/api/blockChain/rawTransaction` 的 `Params` 需要传 protobuf JSON 字符串，不能传 hex 编码的 protobuf bytes。
- `execute_raw_transaction` 和 `send_raw_transaction` 的签名必须基于节点返回的 raw transaction bytes 计算。

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
            serde_json::json!({
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

示例文件保留在 `/examples`，由 `aelf-sdk` crate 显式挂载编译。

```bash
cargo run -p aelf-sdk --example basic_client
cargo run -p aelf-sdk --example wallet_keystore_roundtrip
cargo run -p aelf-sdk --example public_balance
cargo run -p aelf-sdk --example token_transfer
cargo run -p aelf-sdk --example dynamic_contract_get_balance
cargo run -p aelf-sdk --example raw_transaction_flow
```

常用环境变量：

- `AELF_ENDPOINT`
- `AELF_PRIVATE_KEY`
- `AELF_TOKEN_CONTRACT`
- `AELF_TO_ADDRESS`
- `AELF_OWNER_ADDRESS`
- `AELF_AMOUNT`
- `AELF_SEND`

## Feature Flags

v0.1 alpha 故意保持最小表面，目前没有额外的 Cargo feature flag。

- Runtime：仅 Tokio
- TLS：`reqwest + rustls`
- Proto JSON：`pbjson`

## Transport Behavior

`HttpProvider` 默认会对瞬时失败进行自动重试：

- `RetryPolicy::default()` = `2` 次重试，初始退避 `200ms`
- 仅对 `5xx` 响应和传输层临时错误重试
- `4xx` 会立即返回，不进入重试

你可以自定义或关闭重试：

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

dynamic contract 的 descriptor 现在使用内存内 `64` 容量的 LRU cache，避免长时间运行的服务无限增长。

## Local Node Testing

编译整个 workspace：

```bash
cargo check --workspace
cargo check --workspace --examples
```

运行单元测试：

```bash
cargo test --workspace
```

运行本地节点集成测试：

```bash
cargo test -p aelf-sdk --test local_node -- --ignored
```

ignored 测试依赖以下环境变量：

- `AELF_ENDPOINT`
- `AELF_PRIVATE_KEY`
- `AELF_TOKEN_CONTRACT`
- `AELF_TO_ADDRESS`

## Public Node Verification

已在 2026 年 3 月 10 日验证以下公网节点：

- 主链：[https://aelf-public-node.aelf.io/swagger/index.html](https://aelf-public-node.aelf.io/swagger/index.html)
- 侧链：[https://tdvv-public-node.aelf.io/swagger/index.html](https://tdvv-public-node.aelf.io/swagger/index.html)

已验证链路：

- `basic_client` 在主链和侧链都通过
- readonly block / chain / net / tx pool 集成测试在主链和侧链都通过
- Zero -> Token typed contract 调用在主链和侧链都通过
- `contract_at(...).call_json("GetBalance", ...)` 在主链和侧链都通过
- `send_transaction` 已在主链通过
- `create_raw_transaction -> execute_raw_transaction -> send_raw_transaction -> get_transaction_result` 已在主链通过

观察到的兼容性结论：

- dynamic contract 的 JSON 输入输出层会把 `aelf.Address` / `aelf.Hash` 在开发者友好的字符串和 protobuf JSON object 之间做双向归一化。
- `create_raw_transaction` 只有在 `Params` 传 protobuf JSON 时才会成功；如果传 hex 编码的 protobuf bytes，主链公网节点会返回 `403 Invalid params`。
- `execute_raw_transaction` 和 `send_raw_transaction` 的签名必须基于节点生成的 raw transaction bytes，而不是本地构造的 `Transaction` 对象直接复用签名。
- `calculate_transaction_fee` 在公网节点上可能返回空 map，即使交易最终已经被接受并成功出块。

常用命令：

```bash
AELF_ENDPOINT='https://aelf-public-node.aelf.io' cargo run -p aelf-sdk --example basic_client
AELF_ENDPOINT='https://aelf-public-node.aelf.io' AELF_OWNER_ADDRESS='<address>' cargo run -p aelf-sdk --example public_balance
AELF_ENDPOINT='https://aelf-public-node.aelf.io' AELF_PRIVATE_KEY='<private-key>' AELF_TO_ADDRESS='ELF_<address>_AELF' AELF_AMOUNT='1' cargo run -p aelf-sdk --example token_transfer
AELF_ENDPOINT='https://aelf-public-node.aelf.io' AELF_PRIVATE_KEY='<private-key>' AELF_TO_ADDRESS='ELF_<address>_AELF' AELF_AMOUNT='1' cargo run -p aelf-sdk --example raw_transaction_flow
```

## Release

仓库结构：

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

发布流程：

1. 用 `scripts/sync_proto.sh` 同步 upstream proto
2. 执行 `cargo fmt`、`cargo +1.85.0 check --workspace --all-targets --all-features --locked`、`cargo clippy --workspace --all-targets --all-features`、`cargo audit`、`cargo check --workspace --examples`、`cargo test --workspace`
3. 检查 `CHANGELOG.md`，然后先手动运行一次 `publish` GitHub Actions workflow，并把 `dry_run` 设为 `true`
4. 确认 crates.io token 已配置后，再以 `dry_run=false` 重新运行 `publish` workflow

CI 定义在 `.github/workflows/ci.yml`。
发布流程定义在 `.github/workflows/publish.yml`，需要配置仓库 secret `CARGO_REGISTRY_TOKEN`。

MSRV 说明：

- workspace 的 MSRV 现在是 Rust `1.85`。
- CI 已用 `cargo +1.85.0 check --workspace --all-targets --all-features --locked` 做硬性门禁。

## License

MIT
