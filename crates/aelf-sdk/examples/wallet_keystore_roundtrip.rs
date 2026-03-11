use aelf_sdk::{Keystore, Wallet};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let wallet = Wallet::create()?;
    let keystore = Keystore::encrypt_js(&wallet, "123123")?;
    let unlocked = keystore.unlock_js("123123")?;

    println!("address={}", unlocked.address);
    println!("keystore_roundtrip=ok");
    println!("warning=private_key_and_mnemonic_are_intentionally_redacted");
    assert_eq!(wallet.private_key(), unlocked.private_key);
    assert_eq!(wallet.mnemonic(), unlocked.mnemonic);
    Ok(())
}
