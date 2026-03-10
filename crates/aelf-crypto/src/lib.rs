//! Crypto, wallet, signing and address helpers for the AElf Rust SDK.
//!
//! The APIs in this crate align with the reference AElf SDKs and power the
//! facade exported by `aelf-sdk`.

#![forbid(unsafe_code)]

use aelf_proto::aelf::{Address, Hash, Transaction};
use coins_bip32::{
    path::DerivationPath,
    prelude::{RecoveryId, Signature, SigningKey, VerifyingKey},
    xkeys::XPriv,
};
use coins_bip39::{English, Mnemonic, MnemonicError};
use prost::Message;
use rand::rng;
use sha2::{Digest, Sha256};
use std::fmt;
use std::str::FromStr;
use thiserror::Error;
use zeroize::{Zeroize, Zeroizing};

/// Default BIP44 derivation path used by the reference AElf SDKs.
pub const DEFAULT_BIP44_PATH: &str = "m/44'/1616'/0'/0/0";

/// Errors returned by wallet, address and signing helpers.
#[derive(Debug, Error)]
pub enum CryptoError {
    #[error("invalid hex string: {0}")]
    Hex(#[from] hex::FromHexError),
    #[error("invalid mnemonic: {0}")]
    Mnemonic(#[from] MnemonicError),
    #[error("invalid derivation path: {0}")]
    DerivationPath(#[from] coins_bip32::Bip32Error),
    #[error("invalid private key")]
    InvalidPrivateKey,
    #[error("invalid address")]
    InvalidAddress,
    #[error("invalid signature")]
    InvalidSignature,
    #[error("protobuf encode error: {0}")]
    ProtobufEncode(#[from] prost::EncodeError),
    #[error("protobuf decode error: {0}")]
    ProtobufDecode(#[from] prost::DecodeError),
}

/// Wallet created from a mnemonic or raw private key.
///
/// Sensitive fields are zeroized on drop and redacted from `Debug` output.
#[derive(Clone, PartialEq, Eq)]
pub struct Wallet {
    mnemonic: String,
    bip44_path: String,
    private_key: String,
    public_key: String,
    address: String,
}

impl Wallet {
    /// Creates a new wallet using the default AElf BIP44 path.
    pub fn create() -> Result<Self, CryptoError> {
        Self::create_with_path(DEFAULT_BIP44_PATH)
    }

    /// Creates a new wallet using a custom derivation path.
    pub fn create_with_path(path: &str) -> Result<Self, CryptoError> {
        let mut rng = rng();
        let mnemonic = Mnemonic::<English>::from_rng_with_count(&mut rng, 12)?;
        Self::from_mnemonic_with_path(&mnemonic.to_phrase(), path)
    }

    /// Restores a wallet from a mnemonic using the default AElf BIP44 path.
    pub fn from_mnemonic(mnemonic: &str) -> Result<Self, CryptoError> {
        Self::from_mnemonic_with_path(mnemonic, DEFAULT_BIP44_PATH)
    }

    /// Restores a wallet from a mnemonic and custom derivation path.
    pub fn from_mnemonic_with_path(mnemonic: &str, path: &str) -> Result<Self, CryptoError> {
        let mnemonic = Mnemonic::<English>::new_from_phrase(mnemonic)?;
        let path = DerivationPath::from_str(path)?;
        let derived: XPriv = mnemonic.derive_key(path.clone(), None)?;
        let signing_key: SigningKey = <XPriv as AsRef<SigningKey>>::as_ref(&derived).clone();

        Ok(Self::from_signing_key(
            signing_key,
            mnemonic.to_phrase(),
            path.derivation_string(),
        ))
    }

    /// Restores a wallet from a raw secp256k1 private key hex string.
    pub fn from_private_key(private_key_hex: &str) -> Result<Self, CryptoError> {
        let bytes = Zeroizing::new(normalize_private_key_hex(private_key_hex)?);
        let signing_key =
            SigningKey::from_bytes((&*bytes).into()).map_err(|_| CryptoError::InvalidPrivateKey)?;

        Ok(Self::from_signing_key(
            signing_key,
            String::new(),
            DEFAULT_BIP44_PATH.to_owned(),
        ))
    }

    /// Returns the mnemonic phrase, if the wallet was derived from one.
    pub fn mnemonic(&self) -> &str {
        &self.mnemonic
    }

    /// Returns the derivation path used to create the wallet.
    pub fn bip44_path(&self) -> &str {
        &self.bip44_path
    }

    /// Returns the private key as a lowercase hex string.
    pub fn private_key(&self) -> &str {
        &self.private_key
    }

    /// Returns the uncompressed public key as a hex string.
    pub fn public_key(&self) -> &str {
        &self.public_key
    }

    /// Returns the base58 AElf address.
    pub fn address(&self) -> &str {
        &self.address
    }

    /// Reconstructs the signing key from the stored private key.
    pub fn signing_key(&self) -> Result<SigningKey, CryptoError> {
        let bytes = Zeroizing::new(normalize_private_key_hex(&self.private_key)?);
        SigningKey::from_bytes((&*bytes).into()).map_err(|_| CryptoError::InvalidPrivateKey)
    }

    /// Signs arbitrary payload bytes with AElf-compatible recoverable secp256k1.
    pub fn sign(&self, payload: &[u8]) -> Result<Vec<u8>, CryptoError> {
        sign_payload(&self.signing_key()?, payload)
    }

    fn from_signing_key(signing_key: SigningKey, mnemonic: String, bip44_path: String) -> Self {
        let private_key = hex::encode(signing_key.to_bytes());
        let public_key_bytes = signing_key.verifying_key().to_encoded_point(false);
        let public_key = hex::encode(public_key_bytes.as_bytes());
        let address = address_from_public_key(public_key_bytes.as_bytes());

        Self {
            mnemonic,
            bip44_path,
            private_key,
            public_key,
            address,
        }
    }
}

impl fmt::Debug for Wallet {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Wallet")
            .field("mnemonic", &"<redacted>")
            .field("bip44_path", &self.bip44_path)
            .field("private_key", &"<redacted>")
            .field("public_key", &self.public_key)
            .field("address", &self.address)
            .finish()
    }
}

impl Drop for Wallet {
    fn drop(&mut self) {
        self.mnemonic.zeroize();
        self.private_key.zeroize();
    }
}

/// Hashes bytes with SHA-256.
pub fn sha256_bytes(bytes: &[u8]) -> [u8; 32] {
    Sha256::digest(bytes).into()
}

/// Converts an uncompressed public key to an AElf base58 address.
pub fn address_from_public_key(public_key: &[u8]) -> String {
    let first = sha256_bytes(public_key);
    let second = sha256_bytes(&first);
    base58check_encode(&second)
}

/// Converts a base58 or formatted AElf address into protobuf `Address`.
pub fn address_to_pb(address: &str) -> Result<Address, CryptoError> {
    Ok(Address {
        value: decode_address(address)?,
    })
}

/// Converts protobuf `Address` to an AElf base58 address.
pub fn pb_to_address(address: &Address) -> String {
    base58check_encode(&address.value)
}

/// Wraps raw bytes in protobuf `Hash`.
pub fn hash_to_pb(bytes: impl AsRef<[u8]>) -> Hash {
    Hash {
        value: bytes.as_ref().to_vec(),
    }
}

/// Extracts the raw base58 address from either a plain address or a formatted
/// address such as `ELF_<address>_AELF`.
pub fn parse_aelf_address(value: &str) -> &str {
    let mut parts = value.split('_');
    match (parts.next(), parts.next(), parts.next(), parts.next()) {
        (Some(_prefix), Some(address), Some(_chain_id), None) if !address.is_empty() => address,
        _ => value,
    }
}

/// Decodes a base58 or formatted AElf address into its raw bytes.
pub fn decode_address(address: &str) -> Result<Vec<u8>, CryptoError> {
    base58check_decode(parse_aelf_address(address))
}

/// Encodes raw bytes as AElf base58check.
pub fn base58check_encode(payload: &[u8]) -> String {
    let checksum = sha256_bytes(&sha256_bytes(payload));
    let mut bytes = payload.to_vec();
    bytes.extend_from_slice(&checksum[..4]);
    bs58::encode(bytes).into_string()
}

/// Decodes an AElf base58check string into raw bytes.
pub fn base58check_decode(value: &str) -> Result<Vec<u8>, CryptoError> {
    let bytes = bs58::decode(value)
        .into_vec()
        .map_err(|_| CryptoError::InvalidAddress)?;
    if bytes.len() < 4 {
        return Err(CryptoError::InvalidAddress);
    }

    let (payload, checksum) = bytes.split_at(bytes.len() - 4);
    let expected = sha256_bytes(&sha256_bytes(payload));
    if checksum != &expected[..4] {
        return Err(CryptoError::InvalidAddress);
    }

    Ok(payload.to_vec())
}

/// Converts a numeric chain id into the base58 string used by node APIs.
pub fn chain_id_to_base58(chain_id: i32) -> String {
    bs58::encode(chain_id.to_le_bytes()).into_string()
}

/// Converts a base58 chain id string returned by node APIs to its numeric form.
pub fn base58_to_chain_id(chain_id: &str) -> Result<i32, CryptoError> {
    let bytes = bs58::decode(chain_id)
        .into_vec()
        .map_err(|_| CryptoError::InvalidAddress)?;
    let bytes: [u8; 4] = bytes
        .as_slice()
        .try_into()
        .map_err(|_| CryptoError::InvalidAddress)?;
    Ok(i32::from_le_bytes(bytes))
}

/// Calculates the protobuf transaction hash used by AElf.
pub fn transaction_hash(transaction: &Transaction) -> [u8; 32] {
    sha256_bytes(&transaction.encode_to_vec())
}

/// Signs a protobuf transaction with an existing wallet.
pub fn sign_transaction(
    wallet: &Wallet,
    transaction: &Transaction,
) -> Result<Vec<u8>, CryptoError> {
    sign_payload(&wallet.signing_key()?, &transaction.encode_to_vec())
}

/// Signs a protobuf transaction with a raw private key.
pub fn sign_transaction_with_private_key(
    private_key_hex: &str,
    transaction: &Transaction,
) -> Result<Vec<u8>, CryptoError> {
    let wallet = Wallet::from_private_key(private_key_hex)?;
    sign_transaction(&wallet, transaction)
}

/// Verifies a recoverable secp256k1 signature against a payload.
pub fn verify_signature(
    public_key: &[u8],
    payload: &[u8],
    signature_bytes: &[u8],
) -> Result<bool, CryptoError> {
    if signature_bytes.len() != 65 {
        return Err(CryptoError::InvalidSignature);
    }

    let verifying_key =
        VerifyingKey::from_sec1_bytes(public_key).map_err(|_| CryptoError::InvalidSignature)?;
    let recovery_id =
        RecoveryId::from_byte(signature_bytes[64]).ok_or(CryptoError::InvalidSignature)?;
    let signature =
        Signature::from_slice(&signature_bytes[..64]).map_err(|_| CryptoError::InvalidSignature)?;

    let digest = Sha256::new_with_prefix(payload);
    let recovered = VerifyingKey::recover_from_digest(digest, &signature, recovery_id)
        .map_err(|_| CryptoError::InvalidSignature)?;

    Ok(recovered == verifying_key)
}

fn sign_payload(signing_key: &SigningKey, payload: &[u8]) -> Result<Vec<u8>, CryptoError> {
    let digest = Sha256::new_with_prefix(payload);
    let (signature, recovery_id) = signing_key
        .sign_digest_recoverable(digest)
        .map_err(|_| CryptoError::InvalidSignature)?;

    let mut bytes = Vec::with_capacity(65);
    bytes.extend_from_slice(&signature.to_bytes());
    bytes.push(recovery_id.to_byte());
    Ok(bytes)
}

fn normalize_private_key_hex(private_key_hex: &str) -> Result<[u8; 32], CryptoError> {
    let trimmed = private_key_hex
        .strip_prefix("0x")
        .unwrap_or(private_key_hex);
    let normalized = if trimmed.len() >= 64 {
        trimmed.to_owned()
    } else {
        format!("{trimmed:0>64}")
    };

    let bytes = hex::decode(normalized)?;
    bytes
        .as_slice()
        .try_into()
        .map_err(|_| CryptoError::InvalidPrivateKey)
}

#[cfg(test)]
mod tests {
    use super::{
        base58_to_chain_id, chain_id_to_base58, decode_address, parse_aelf_address,
        sign_transaction, verify_signature, Wallet,
    };
    use aelf_proto::aelf::Transaction;
    use prost::Message;

    const JS_ADDRESS_MNEMONIC: &str =
        "history segment pizza all time regret robust animal loud gasp razor gadget";
    const JS_EXPECTED_ADDRESS: &str = "CTqD1M6Kt2v2jS8QLR6tcTq7vv9dHsKibUr6BEaN3BZ94i92m";
    const JS_SIGN_PRIVATE_KEY: &str =
        "03bd0cea9730bcfc8045248fd7f4841ea19315995c44801a3dfede0ca872f808";
    const JS_SIGN_EXPECTED: &str =
        "276aa36fcab0ac3d4071a4bfb868f636d1a9639916afe4ec329529014f923a372b688b4eb59d6587481bc15e4a1684e1d92b7598967767713d1504dcea83dadb01";
    const TEST_MNEMONIC: &str =
        "orange learn result add snack curtain double state expose bless also clarify";
    const TEST_PRIVATE_KEY: &str =
        "cc2895b46707a34eefd3c61bd4a8487266e0398a93309a9910a2b88e587b6582";

    #[test]
    fn derives_known_private_key_from_mnemonic() {
        let wallet = Wallet::from_mnemonic(TEST_MNEMONIC).expect("wallet");
        assert_eq!(wallet.private_key(), TEST_PRIVATE_KEY);
    }

    #[test]
    fn derives_same_address_from_private_key_and_mnemonic() {
        let from_mnemonic = Wallet::from_mnemonic(TEST_MNEMONIC).expect("wallet");
        let from_private_key = Wallet::from_private_key(TEST_PRIVATE_KEY).expect("wallet");
        assert_eq!(from_mnemonic.address(), from_private_key.address());
    }

    #[test]
    fn chain_id_roundtrip() {
        let encoded = chain_id_to_base58(9_992_731);
        let decoded = base58_to_chain_id(&encoded).expect("chain id");
        assert_eq!(decoded, 9_992_731);
    }

    #[test]
    fn transaction_signature_is_recoverable() {
        let wallet = Wallet::from_private_key(TEST_PRIVATE_KEY).expect("wallet");
        let transaction = Transaction {
            from: None,
            to: None,
            ref_block_number: 1,
            ref_block_prefix: vec![1, 2, 3, 4],
            method_name: "Test".to_owned(),
            params: vec![1, 2, 3],
            signature: Vec::new(),
        };

        let signature = sign_transaction(&wallet, &transaction).expect("signature");
        let public_key = hex::decode(wallet.public_key()).expect("public key");

        assert!(
            verify_signature(&public_key, &transaction.encode_to_vec(), &signature)
                .expect("verify")
        );
    }

    #[test]
    fn derives_known_address_from_js_mnemonic_fixture() {
        let wallet = Wallet::from_mnemonic(JS_ADDRESS_MNEMONIC).expect("wallet");
        assert_eq!(wallet.address(), JS_EXPECTED_ADDRESS);
    }

    #[test]
    fn signs_payload_compatible_with_js_fixture() {
        let wallet = Wallet::from_private_key(JS_SIGN_PRIVATE_KEY).expect("wallet");
        let signature = wallet.sign(b"hello world").expect("signature");
        assert_eq!(hex::encode(signature), JS_SIGN_EXPECTED);
    }

    #[test]
    fn parses_formatted_aelf_address() {
        let formatted = format!("ELF_{JS_EXPECTED_ADDRESS}_AELF");
        assert_eq!(parse_aelf_address(&formatted), JS_EXPECTED_ADDRESS);
        assert_eq!(
            decode_address(&formatted).expect("formatted address"),
            decode_address(JS_EXPECTED_ADDRESS).expect("raw address")
        );
    }

    #[test]
    fn debug_redacts_wallet_secrets() {
        let wallet = Wallet::from_mnemonic(TEST_MNEMONIC).expect("wallet");
        let debug = format!("{wallet:?}");
        assert!(!debug.contains(TEST_MNEMONIC));
        assert!(!debug.contains(TEST_PRIVATE_KEY));
        assert!(debug.contains(wallet.address()));
    }
}
