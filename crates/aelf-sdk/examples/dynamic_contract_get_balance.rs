use aelf_sdk::{AElfClient, ClientConfig, Wallet};
use serde_json::json;

#[tokio::main(flavor = "current_thread")]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let endpoint =
        std::env::var("AELF_ENDPOINT").unwrap_or_else(|_| "http://127.0.0.1:8000".to_owned());
    let token_address = std::env::var("AELF_TOKEN_CONTRACT")?;
    let owner = std::env::var("AELF_OWNER_ADDRESS")?;
    let private_key = std::env::var("AELF_PRIVATE_KEY").unwrap_or_else(|_| {
        "0000000000000000000000000000000000000000000000000000000000000001".to_owned()
    });

    let client = AElfClient::new(ClientConfig::new(endpoint))?;
    let wallet = Wallet::from_private_key(&private_key)?;
    let contract = client.contract_at(token_address, wallet).await?;
    let balance = contract
        .call_json(
            "GetBalance",
            json!({
                "symbol": "ELF",
                "owner": owner,
            }),
        )
        .await?;

    println!("{balance}");
    Ok(())
}
