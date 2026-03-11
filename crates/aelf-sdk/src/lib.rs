//! High-level facade for the AElf Rust SDK.
//!
//! This crate re-exports the most commonly used client, wallet, keystore and
//! contract APIs behind a single dependency.

#![forbid(unsafe_code)]

pub use aelf_client::config::{BasicAuth, ClientConfig, RetryPolicy};
pub use aelf_client::dto;
#[cfg(feature = "native-http")]
pub use aelf_client::provider::HttpProvider;
pub use aelf_client::provider::Provider;
pub use aelf_client::{AElfError, KeyPairInfo, TransactionBuilder};
pub use aelf_contract::{
    AedposContract, ContractError, CrossChainContract, DynamicContract, ElectionContract,
    TokenContract, VoteContract, ZeroContract,
};
pub use aelf_crypto::{
    address_from_public_key, address_to_pb, base58_to_chain_id, chain_id_to_base58, decode_address,
    hash_to_pb, parse_aelf_address, pb_to_address, sign_transaction,
    sign_transaction_with_private_key, transaction_hash, CryptoError, Wallet, DEFAULT_BIP44_PATH,
};
pub use aelf_keystore::{Keystore, KeystoreEncryptOptions, KeystoreError, UnlockedKeystore};
pub use aelf_proto as proto;

/// High-level async client that mirrors the main entry point of the other AElf SDKs.
#[derive(Clone)]
pub struct AElfClient {
    inner: aelf_client::AElfClient,
}

impl AElfClient {
    /// Creates a facade client backed by the default HTTP provider.
    #[cfg(feature = "native-http")]
    pub fn new(config: ClientConfig) -> Result<Self, AElfError> {
        Ok(Self {
            inner: aelf_client::AElfClient::new(config)?,
        })
    }

    /// Creates a facade client from a custom provider implementation.
    pub fn with_provider<P>(provider: P) -> Result<Self, AElfError>
    where
        P: Provider + 'static,
    {
        Ok(Self {
            inner: aelf_client::AElfClient::with_provider(provider)?,
        })
    }

    /// Returns block-related APIs.
    pub fn block(&self) -> aelf_client::BlockService {
        self.inner.block()
    }

    /// Returns chain-related APIs.
    pub fn chain(&self) -> aelf_client::ChainService {
        self.inner.chain()
    }

    /// Returns network-related APIs.
    pub fn net(&self) -> aelf_client::NetService {
        self.inner.net()
    }

    /// Returns transaction-related APIs.
    pub fn tx(&self) -> aelf_client::TransactionService {
        self.inner.tx()
    }

    /// Returns local utility helpers.
    pub fn utils(&self) -> aelf_client::ClientUtilsService {
        self.inner.utils()
    }

    /// Creates a transaction builder bound to this client.
    pub fn transaction_builder(&self) -> TransactionBuilder {
        self.inner.transaction_builder()
    }

    /// Loads a contract descriptor from chain and returns a dynamic contract handle.
    pub async fn contract_at(
        &self,
        address: impl Into<String>,
        wallet: Wallet,
    ) -> Result<DynamicContract, ContractError> {
        DynamicContract::at(self.inner.clone(), address.into(), wallet).await
    }

    /// Returns a typed wrapper for the genesis zero contract.
    pub fn zero_contract(&self, address: impl Into<String>, wallet: Wallet) -> ZeroContract {
        ZeroContract::new(self.inner.clone(), wallet, address)
    }

    /// Returns a typed wrapper for the token contract.
    pub fn token_contract(&self, address: impl Into<String>, wallet: Wallet) -> TokenContract {
        TokenContract::new(self.inner.clone(), wallet, address)
    }

    /// Returns a typed wrapper for the election contract.
    pub fn election_contract(
        &self,
        address: impl Into<String>,
        wallet: Wallet,
    ) -> ElectionContract {
        ElectionContract::new(self.inner.clone(), wallet, address)
    }

    /// Returns a typed wrapper for the vote contract.
    pub fn vote_contract(&self, address: impl Into<String>, wallet: Wallet) -> VoteContract {
        VoteContract::new(self.inner.clone(), wallet, address)
    }

    /// Returns a typed wrapper for the cross-chain contract.
    pub fn cross_chain_contract(
        &self,
        address: impl Into<String>,
        wallet: Wallet,
    ) -> CrossChainContract {
        CrossChainContract::new(self.inner.clone(), wallet, address)
    }

    /// Returns a typed wrapper for the AEDPoS contract.
    pub fn aedpos_contract(&self, address: impl Into<String>, wallet: Wallet) -> AedposContract {
        AedposContract::new(self.inner.clone(), wallet, address)
    }

    /// Exposes the lower-level client for advanced use cases.
    pub fn inner(&self) -> &aelf_client::AElfClient {
        &self.inner
    }
}

/// Formats a raw token amount using the token's decimals.
pub fn format_token_amount(amount: i64, decimals: i32) -> String {
    if decimals <= 0 {
        return amount.to_string();
    }

    let negative = amount.is_negative();
    let digits = amount.unsigned_abs().to_string();
    let scale = decimals as usize;
    let formatted = if digits.len() <= scale {
        let mut value = String::from("0.");
        value.push_str(&"0".repeat(scale - digits.len()));
        value.push_str(&digits);
        value
    } else {
        let split = digits.len() - scale;
        format!("{}.{}", &digits[..split], &digits[split..])
    };

    if negative {
        format!("-{formatted}")
    } else {
        formatted
    }
}

#[cfg(test)]
mod tests {
    use super::format_token_amount;

    #[test]
    fn formats_token_amount_with_decimals() {
        assert_eq!(format_token_amount(123_456_789, 8), "1.23456789");
        assert_eq!(format_token_amount(1, 8), "0.00000001");
        assert_eq!(format_token_amount(-5, 2), "-0.05");
    }
}
