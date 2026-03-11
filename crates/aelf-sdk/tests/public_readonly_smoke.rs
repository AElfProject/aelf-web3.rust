use aelf_sdk::proto::token::GetBalanceInput;
use aelf_sdk::{address_to_pb, AElfClient, ClientConfig, Wallet};
use serde_json::json;
use std::error::Error;

const READONLY_PRIVATE_KEY: &str =
    "0000000000000000000000000000000000000000000000000000000000000001";
const MAIN_CHAIN_ENDPOINT: &str = "https://aelf-public-node.aelf.io";
const SIDE_CHAIN_ENDPOINT: &str = "https://tdvv-public-node.aelf.io";

async fn assert_public_readonly_smoke(
    endpoint: &str,
    expected_chain_id: &str,
) -> Result<(), Box<dyn Error>> {
    let client = AElfClient::new(ClientConfig::new(endpoint))?;
    let wallet = Wallet::from_private_key(READONLY_PRIVATE_KEY)?;

    let status = client.chain().get_chain_status().await?;
    assert_eq!(status.chain_id, expected_chain_id);
    assert!(status.best_chain_height > 0);
    assert!(!status.genesis_contract_address.is_empty());

    let token_address = client
        .utils()
        .get_contract_address_by_name("AElf.ContractNames.Token")
        .await?;
    assert!(!token_address.is_empty());

    let token = client.token_contract(token_address.clone(), wallet.clone());
    let primary_symbol = token.get_primary_token_symbol().await?;
    let native = token.get_native_token_info().await?;
    assert_eq!(native.symbol, primary_symbol);

    let balance = token
        .get_balance(&GetBalanceInput {
            symbol: primary_symbol.clone(),
            owner: Some(address_to_pb(wallet.address())?),
        })
        .await?;
    assert_eq!(balance.symbol, primary_symbol);

    let dynamic = client.contract_at(token_address, wallet.clone()).await?;
    let dynamic_balance = dynamic
        .call_json(
            "GetBalance",
            json!({
                "symbol": balance.symbol,
                "owner": wallet.address(),
            }),
        )
        .await?;
    assert_eq!(dynamic_balance.get("owner"), Some(&json!(wallet.address())));

    Ok(())
}

#[tokio::test(flavor = "current_thread")]
#[ignore = "requires live public nodes"]
async fn public_main_chain_readonly_smoke() -> Result<(), Box<dyn Error>> {
    assert_public_readonly_smoke(MAIN_CHAIN_ENDPOINT, "AELF").await
}

#[tokio::test(flavor = "current_thread")]
#[ignore = "requires live public nodes"]
async fn public_side_chain_readonly_smoke() -> Result<(), Box<dyn Error>> {
    assert_public_readonly_smoke(SIDE_CHAIN_ENDPOINT, "tDVV").await
}
