use aelf_sdk::proto::aelf::Address;
use aelf_sdk::proto::token::GetBalanceInput;
use aelf_sdk::{decode_address, format_token_amount, AElfClient, ClientConfig, Wallet};

const READONLY_PRIVATE_KEY: &str =
    "0000000000000000000000000000000000000000000000000000000000000001";

#[tokio::main(flavor = "current_thread")]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let endpoint =
        std::env::var("AELF_ENDPOINT").unwrap_or_else(|_| "http://127.0.0.1:8000".to_owned());
    let owner = std::env::var("AELF_OWNER_ADDRESS")?;
    let client = AElfClient::new(ClientConfig::new(endpoint))?;
    let wallet = Wallet::from_private_key(READONLY_PRIVATE_KEY)?;
    let token_address = client
        .utils()
        .get_contract_address_by_name("AElf.ContractNames.Token")
        .await?;
    let token = client.token_contract(token_address.clone(), wallet);
    let token_info = token.get_native_token_info().await?;
    let balance = token
        .get_balance(&GetBalanceInput {
            symbol: token_info.symbol.clone(),
            owner: Some(Address {
                value: decode_address(&owner)?,
            }),
        })
        .await?;

    println!("token_contract={token_address}");
    println!("symbol={}", token_info.symbol);
    println!("decimals={}", token_info.decimals);
    println!("raw_balance={}", balance.balance);
    println!(
        "display_balance={}",
        format_token_amount(balance.balance, token_info.decimals)
    );
    Ok(())
}
