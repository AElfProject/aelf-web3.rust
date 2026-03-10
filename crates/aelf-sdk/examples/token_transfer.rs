use aelf_sdk::proto::token::TransferInput;
use aelf_sdk::{decode_address, parse_aelf_address, AElfClient, ClientConfig, Wallet};
use prost::Message;
use tokio::time::{sleep, Duration};

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let endpoint =
        std::env::var("AELF_ENDPOINT").unwrap_or_else(|_| "http://127.0.0.1:8000".to_owned());
    let private_key = std::env::var("AELF_PRIVATE_KEY")?;
    let to = std::env::var("AELF_TO_ADDRESS")?;
    let amount = std::env::var("AELF_AMOUNT")
        .ok()
        .map(|value| value.parse::<i64>())
        .transpose()?
        .unwrap_or(1);
    let token_address = match std::env::var("AELF_TOKEN_CONTRACT") {
        Ok(value) if !value.is_empty() => value,
        _ => String::new(),
    };
    let should_send = matches!(
        std::env::var("AELF_SEND").ok().as_deref(),
        Some("1" | "true" | "TRUE" | "yes" | "YES")
    );
    let max_fee = std::env::var("AELF_MAX_FEE")
        .ok()
        .map(|value| value.parse::<f64>())
        .transpose()?
        .unwrap_or(0.01_f64);

    let client = AElfClient::new(ClientConfig::new(endpoint))?;
    let wallet = Wallet::from_private_key(&private_key)?;
    let to = parse_aelf_address(&to).to_owned();
    let token_address = if token_address.is_empty() {
        client
            .utils()
            .get_contract_address_by_name("AElf.ContractNames.Token")
            .await?
    } else {
        token_address
    };
    let formatted = client.utils().get_formatted_address(&to).await?;
    let input = TransferInput {
        to: Some(aelf_sdk::proto::aelf::Address {
            value: decode_address(&to)?,
        }),
        symbol: "ELF".to_owned(),
        amount,
        memo: "transfer from rust sdk example".to_owned(),
    };
    let transaction = client
        .transaction_builder()
        .with_wallet(wallet.clone())
        .with_contract(token_address)
        .with_method("Transfer")
        .with_message(&input)
        .build_signed()
        .await?;
    let raw_transaction = hex::encode(transaction.encode_to_vec());
    let fee = client
        .tx()
        .calculate_transaction_fee(&aelf_sdk::dto::CalculateTransactionFeeInput {
            raw_transaction: raw_transaction.clone(),
        })
        .await?;
    let elf_fee = fee.transaction_fee.get("ELF").copied().unwrap_or_default();

    println!("from={}", wallet.address());
    println!("to={to}");
    println!("formatted_to={formatted}");
    println!("amount_raw={amount}");
    println!("estimated_fee={:?}", fee.transaction_fee);

    if !should_send {
        println!("dry_run=true");
        return Ok(());
    }

    if elf_fee > max_fee {
        return Err(format!("estimated ELF fee {elf_fee} exceeds max fee {max_fee}").into());
    }

    let output = client.tx().send_transaction(&raw_transaction).await?;
    println!("transaction_id={}", output.transaction_id);

    for _ in 0..15 {
        let result = client
            .tx()
            .get_transaction_result(&output.transaction_id)
            .await?;
        println!("status={}", result.status);
        if result.status != "PENDING" {
            println!("tx_fee={:?}", result.get_transaction_fees());
            if !result.error.is_empty() {
                println!("error={}", result.error);
            }
            break;
        }
        sleep(Duration::from_secs(1)).await;
    }

    Ok(())
}
