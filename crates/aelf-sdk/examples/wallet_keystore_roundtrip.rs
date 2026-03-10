use aelf_sdk::{Keystore, Wallet};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let wallet = Wallet::create()?;
    let keystore = Keystore::encrypt_js(&wallet, "123123")?;
    let unlocked = keystore.unlock_js("123123")?;

    println!("address={}", unlocked.address);
    println!("private_key={}", unlocked.private_key);
    println!("mnemonic={}", unlocked.mnemonic);
    Ok(())
}
