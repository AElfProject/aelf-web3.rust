use aelf_sdk::proto::token::TransferInput;
use aelf_sdk::{address_to_pb, parse_aelf_address, AElfClient, ClientConfig, Wallet};
use prost::Message;
use std::env;
use std::error::Error;
use tokio::time::{sleep, Duration};

fn required_env(name: &str) -> Result<String, Box<dyn Error>> {
    env::var(name).map_err(|_| format!("missing required environment variable: {name}").into())
}

#[tokio::test(flavor = "current_thread")]
#[ignore = "requires funded account and reachable node"]
async fn funded_send_transaction_smoke() -> Result<(), Box<dyn Error>> {
    let endpoint = required_env("AELF_ENDPOINT")?;
    let private_key = required_env("AELF_PRIVATE_KEY")?;
    let to_address = required_env("AELF_TO_ADDRESS")?;
    let amount = env::var("AELF_AMOUNT")
        .ok()
        .map(|value| value.parse::<i64>())
        .transpose()?
        .unwrap_or(1);

    let client = AElfClient::new(ClientConfig::new(endpoint))?;
    let wallet = Wallet::from_private_key(&private_key)?;
    let token_address = match env::var("AELF_TOKEN_CONTRACT") {
        Ok(value) if !value.is_empty() => value,
        _ => {
            client
                .utils()
                .get_contract_address_by_name("AElf.ContractNames.Token")
                .await?
        }
    };

    let transfer = TransferInput {
        to: Some(address_to_pb(parse_aelf_address(&to_address))?),
        symbol: "ELF".to_owned(),
        amount,
        memo: "sdk funded transaction smoke".to_owned(),
    };

    let transaction = client
        .transaction_builder()
        .with_wallet(wallet)
        .with_contract(token_address)
        .with_method("Transfer")
        .with_message(&transfer)
        .build_signed()
        .await?;
    let raw_transaction = hex::encode(transaction.encode_to_vec());

    let send_output = client.tx().send_transaction(&raw_transaction).await?;
    assert!(!send_output.transaction_id.is_empty());

    for _ in 0..20 {
        let result = client
            .tx()
            .get_transaction_result(&send_output.transaction_id)
            .await?;
        if result.status == "MINED" {
            return Ok(());
        }
        if result.status != "PENDING" {
            return Err(
                format!("transaction ended in unexpected status: {}", result.status).into(),
            );
        }

        sleep(Duration::from_secs(1)).await;
    }

    Err("transaction was not mined before timeout".into())
}
