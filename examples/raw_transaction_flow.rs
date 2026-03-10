use aelf_sdk::dto::{
    CalculateTransactionFeeInput, CreateRawTransactionInput, ExecuteRawTransactionDto,
    SendRawTransactionInput,
};
use aelf_sdk::proto::token::TransferInput;
use aelf_sdk::{decode_address, parse_aelf_address, AElfClient, ClientConfig, Wallet};
use prost::Message;
use tokio::time::{sleep, Duration};

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let endpoint =
        std::env::var("AELF_ENDPOINT").unwrap_or_else(|_| "http://127.0.0.1:8000".to_owned());
    let private_key = std::env::var("AELF_PRIVATE_KEY")?;
    let to_env = std::env::var("AELF_TO_ADDRESS")?;
    let to = parse_aelf_address(&to_env).to_owned();
    let amount = std::env::var("AELF_AMOUNT")
        .ok()
        .map(|value| value.parse::<i64>())
        .transpose()?
        .unwrap_or(1);
    let should_send = matches!(
        std::env::var("AELF_SEND").ok().as_deref(),
        Some("1" | "true" | "TRUE" | "yes" | "YES")
    );
    let client = AElfClient::new(ClientConfig::new(endpoint))?;
    let wallet = Wallet::from_private_key(&private_key)?;
    let token_address = client
        .utils()
        .get_contract_address_by_name("AElf.ContractNames.Token")
        .await?;
    let chain_status = client.chain().get_chain_status().await?;
    let transfer = TransferInput {
        to: Some(aelf_sdk::proto::aelf::Address {
            value: decode_address(&to)?,
        }),
        symbol: "ELF".to_owned(),
        amount,
        memo: "raw transaction flow validation".to_owned(),
    };
    let params_json = serde_json::to_string(&transfer)?;

    let unsigned = client
        .utils()
        .generate_transaction(wallet.address(), &token_address, "Transfer", &transfer)
        .await?;
    let signed = client.utils().sign_transaction(&wallet, unsigned)?;
    let mut unsigned_only = signed.clone();
    unsigned_only.signature.clear();
    let unsigned_raw = hex::encode(unsigned_only.encode_to_vec());
    let created = client
        .tx()
        .create_raw_transaction(&CreateRawTransactionInput {
            from: wallet.address().to_owned(),
            to: token_address.clone(),
            ref_block_number: chain_status.best_chain_height,
            ref_block_hash: chain_status.best_chain_hash.clone(),
            method_name: "Transfer".to_owned(),
            params: params_json,
        })
        .await;
    let (raw_transaction, created_by_node) = match created {
        Ok(created) => (created.raw_transaction, true),
        Err(error) => {
            println!("create_raw_transaction_error={error}");
            (unsigned_raw.clone(), false)
        }
    };
    let signature = hex::encode(wallet.sign(&hex::decode(&raw_transaction)?)?);

    println!("from={}", wallet.address());
    println!("to={to}");
    println!(
        "formatted_to={}",
        client.utils().get_formatted_address(&to).await?
    );
    println!("token_contract={token_address}");
    println!("amount_raw={amount}");
    println!("created_by_node={created_by_node}");
    println!(
        "raw_transaction_matches_sdk={}",
        raw_transaction.eq_ignore_ascii_case(&unsigned_raw)
    );

    let execute_raw = client
        .tx()
        .execute_raw_transaction(&ExecuteRawTransactionDto {
            raw_transaction: raw_transaction.clone(),
            signature: signature.clone(),
        })
        .await?;
    println!("execute_raw_result={execute_raw}");

    let fee = client
        .tx()
        .calculate_transaction_fee(&CalculateTransactionFeeInput {
            raw_transaction: raw_transaction.clone(),
        })
        .await?;
    println!("estimated_fee={:?}", fee.transaction_fee);

    if !should_send {
        println!("dry_run=true");
        return Ok(());
    }

    let sent = client
        .tx()
        .send_raw_transaction(&SendRawTransactionInput {
            transaction: raw_transaction,
            signature,
            return_transaction: true,
        })
        .await?;
    println!("transaction_id={}", sent.transaction_id);

    for _ in 0..15 {
        let result = client
            .tx()
            .get_transaction_result(&sent.transaction_id)
            .await?;
        println!("status={}", result.status);
        if result.status != "PENDING" {
            println!("tx_fee={:?}", result.get_transaction_fees());
            println!("block_number={}", result.block_number);
            if !result.error.is_empty() {
                println!("error={}", result.error);
            }
            break;
        }
        sleep(Duration::from_secs(1)).await;
    }

    Ok(())
}
