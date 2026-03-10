use aelf_sdk::dto::{
    CalculateTransactionFeeInput, CreateRawTransactionInput, ExecuteRawTransactionDto,
    SendRawTransactionInput,
};
use aelf_sdk::proto::token::GetBalanceInput;
use aelf_sdk::proto::token::TransferInput;
use aelf_sdk::{decode_address, AElfClient, ClientConfig, Wallet};
use prost::Message;
use serde_json::json;
use std::env;
use std::error::Error;
use tokio::time::{sleep, Duration};

const READONLY_PRIVATE_KEY: &str =
    "0000000000000000000000000000000000000000000000000000000000000001";

fn endpoint() -> String {
    env::var("AELF_ENDPOINT").unwrap_or_else(|_| "http://127.0.0.1:8000".to_owned())
}

fn required_env(name: &str) -> String {
    match env::var(name) {
        Ok(value) => value,
        Err(_) => panic!("missing required environment variable: {name}"),
    }
}

fn readonly_wallet() -> Wallet {
    Wallet::from_private_key(READONLY_PRIVATE_KEY).expect("readonly wallet")
}

#[tokio::test]
#[ignore = "requires a local AElf node"]
async fn local_node_readonly_endpoints() -> Result<(), Box<dyn Error>> {
    let client = AElfClient::new(ClientConfig::new(endpoint()))?;

    let height = client.block().get_block_height().await?;
    let status = client.chain().get_chain_status().await?;
    let peers = client.net().get_peers(false).await?;
    let network = client.net().get_network_info().await?;
    let pool = client.tx().get_transaction_pool_status().await?;

    assert!(height >= 0);
    assert!(!status.best_chain_hash.is_empty());
    assert!(!network.version.is_empty() || !network.extra.is_empty());
    assert!(pool.queued >= 0);
    assert!(peers
        .iter()
        .all(|peer| !peer.ip_address.is_empty() || !peer.extra.is_empty()));

    Ok(())
}

#[tokio::test]
#[ignore = "requires a reachable AElf node with readonly APIs"]
async fn readonly_contract_endpoints() -> Result<(), Box<dyn Error>> {
    let client = AElfClient::new(ClientConfig::new(endpoint()))?;
    let wallet = readonly_wallet();
    let chain_status = client.chain().get_chain_status().await?;
    let zero = client.zero_contract(
        chain_status.genesis_contract_address.clone(),
        wallet.clone(),
    );

    let token_address_from_utils = client
        .utils()
        .get_contract_address_by_name("AElf.ContractNames.Token")
        .await?;
    let token_address_from_zero = zero
        .get_contract_address_by_name("AElf.ContractNames.Token")
        .await?;
    assert_eq!(token_address_from_utils, token_address_from_zero);

    let token = client.token_contract(token_address_from_utils.clone(), wallet.clone());
    let primary_symbol = token.get_primary_token_symbol().await?;
    let native = token.get_native_token_info().await?;
    let owner = aelf_sdk::proto::aelf::Address {
        value: decode_address(wallet.address())?,
    };
    let balance = token
        .get_balance(&GetBalanceInput {
            symbol: primary_symbol.clone(),
            owner: Some(owner.clone()),
        })
        .await?;
    assert_eq!(native.symbol, primary_symbol);
    assert_eq!(balance.symbol, primary_symbol);

    let dynamic = client
        .contract_at(token_address_from_utils, wallet.clone())
        .await?;
    let dynamic_balance = dynamic
        .call_json(
            "GetBalance",
            json!({
                "symbol": primary_symbol,
                "owner": wallet.address(),
            }),
        )
        .await?;
    assert_eq!(dynamic_balance.get("symbol"), Some(&json!(balance.symbol)));
    assert_eq!(dynamic_balance.get("owner"), Some(&json!(wallet.address())));

    Ok(())
}

#[tokio::test]
#[ignore = "requires a local AElf node and funded account"]
async fn local_node_transaction_endpoints() -> Result<(), Box<dyn Error>> {
    let client = AElfClient::new(ClientConfig::new(endpoint()))?;
    let private_key = required_env("AELF_PRIVATE_KEY");
    let to = required_env("AELF_TO_ADDRESS");
    let token_contract = required_env("AELF_TOKEN_CONTRACT");

    let wallet = Wallet::from_private_key(&private_key)?;
    let chain_status = client.chain().get_chain_status().await?;

    let transfer = TransferInput {
        to: Some(aelf_sdk::proto::aelf::Address {
            value: decode_address(&to)?,
        }),
        symbol: "ELF".to_owned(),
        amount: 1,
        memo: "sdk local node integration test".to_owned(),
    };
    let transfer_params = serde_json::to_string(&transfer)?;

    let create_output = client
        .tx()
        .create_raw_transaction(&CreateRawTransactionInput {
            from: wallet.address().to_owned(),
            to: token_contract.clone(),
            ref_block_number: chain_status.best_chain_height,
            ref_block_hash: chain_status.best_chain_hash.clone(),
            method_name: "Transfer".to_owned(),
            params: transfer_params,
        })
        .await?;
    assert!(!create_output.raw_transaction.is_empty());

    let unsigned = client
        .utils()
        .generate_transaction(wallet.address(), &token_contract, "Transfer", &transfer)
        .await?;
    let signed = client.utils().sign_transaction(&wallet, unsigned)?;
    let raw_transaction = hex::encode(signed.encode_to_vec());
    let signature = hex::encode(wallet.sign(&hex::decode(&create_output.raw_transaction)?)?);

    let execute_result = client.tx().execute_transaction(&raw_transaction).await?;
    assert!(!execute_result.is_empty());

    let execute_raw_result = client
        .tx()
        .execute_raw_transaction(&ExecuteRawTransactionDto {
            raw_transaction: create_output.raw_transaction.clone(),
            signature: signature.clone(),
        })
        .await?;
    assert!(!execute_raw_result.is_empty());

    let fee = client
        .tx()
        .calculate_transaction_fee(&CalculateTransactionFeeInput {
            raw_transaction: create_output.raw_transaction.clone(),
        })
        .await?;
    assert!(fee.success);

    let send_output = client
        .tx()
        .send_raw_transaction(&SendRawTransactionInput {
            transaction: create_output.raw_transaction,
            signature,
            return_transaction: true,
        })
        .await?;
    assert!(!send_output.transaction_id.is_empty());

    let batch = serde_json::to_string(&vec![raw_transaction.clone()])?;
    let transaction_ids = client.tx().send_transactions(&batch).await?;
    assert!(!transaction_ids.is_empty());

    for _ in 0..10 {
        let result = client
            .tx()
            .get_transaction_result(&send_output.transaction_id)
            .await?;
        if result.status != "PENDING" {
            let merkle = client
                .tx()
                .get_merkle_path_by_transaction_id(&send_output.transaction_id)
                .await?;
            assert!(!merkle.merkle_path_nodes.is_empty());
            return Ok(());
        }

        sleep(Duration::from_secs(1)).await;
    }

    Err("transaction was not mined before timeout".into())
}
