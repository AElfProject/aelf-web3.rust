use aelf_sdk::{AElfClient, ClientConfig};

#[tokio::main(flavor = "current_thread")]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let endpoint =
        std::env::var("AELF_ENDPOINT").unwrap_or_else(|_| "http://127.0.0.1:8000".to_owned());
    let client = AElfClient::new(ClientConfig::new(endpoint))?;
    let status = client.chain().get_chain_status().await?;
    println!(
        "chain_id={} best_height={} genesis={}",
        status.chain_id, status.best_chain_height, status.genesis_contract_address
    );
    Ok(())
}
