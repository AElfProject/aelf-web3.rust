//! JS-compatible keystore encryption and decryption for AElf wallets.

#![forbid(unsafe_code)]

use aelf_crypto::Wallet;
use aes::{Aes128, Aes192, Aes256};
use cbc::{Decryptor as CbcDecryptor, Encryptor as CbcEncryptor};
use cipher::block_padding::Pkcs7;
use cipher::{BlockDecryptMut, BlockEncryptMut, InvalidLength, KeyIvInit, StreamCipher};
use core::cmp;
use ctr::Ctr128BE;
use pbkdf2::pbkdf2_hmac;
use rand::{rng, RngCore};
use salsa20::{
    cipher::{typenum::U4, StreamCipherCore},
    SalsaCore,
};
use scrypt::{scrypt, Params};
use serde::{Deserialize, Serialize};
use sha3::{Digest, Keccak256};
use std::fmt;
use thiserror::Error;
use zeroize::{Zeroize, Zeroizing};

const DEFAULT_DKLEN: usize = 32;
const DEFAULT_N: u32 = 8192;
const DEFAULT_R: u32 = 8;
const DEFAULT_P: u32 = 1;
const DEFAULT_CIPHER: &str = "aes-128-ctr";

/// Errors returned while encoding or decoding AElf keystores.
#[derive(Debug, Error)]
pub enum KeystoreError {
    #[error("invalid scrypt params")]
    InvalidScryptParams,
    #[error("unsupported cipher: {0}")]
    UnsupportedCipher(String),
    #[error("cipher key or iv length mismatch")]
    InvalidCipherLength(#[from] InvalidLength),
    #[error("invalid password")]
    InvalidPassword,
    #[error("cipher padding error")]
    CipherPadding,
    #[error("hex decode error: {0}")]
    Hex(#[from] hex::FromHexError),
    #[error("json error: {0}")]
    Json(#[from] serde_json::Error),
}

/// Serializable keystore compatible with `aelf-web3.js`.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Keystore {
    pub version: u32,
    #[serde(rename = "type", default)]
    pub kind: String,
    #[serde(rename = "nickName", default)]
    pub nick_name: String,
    #[serde(default)]
    pub id: Option<String>,
    #[serde(default)]
    pub address: String,
    pub crypto: KeystoreCrypto,
}

/// Cryptographic payload stored in an AElf keystore file.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct KeystoreCrypto {
    pub cipher: String,
    pub ciphertext: String,
    pub cipherparams: CipherParams,
    #[serde(rename = "mnemonicEncrypted", default)]
    pub mnemonic_encrypted: String,
    pub kdf: String,
    pub kdfparams: KdfParams,
    pub mac: String,
}

/// IV or nonce parameters for the selected cipher.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct CipherParams {
    pub iv: String,
}

/// Scrypt parameters used to derive the encryption key.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct KdfParams {
    pub r: u32,
    pub n: u32,
    pub p: u32,
    #[serde(alias = "dkLen")]
    pub dklen: usize,
    pub salt: String,
}

/// Decrypted keystore contents.
///
/// Sensitive fields are zeroized on drop and redacted from `Debug` output.
#[derive(Clone, PartialEq, Eq)]
pub struct UnlockedKeystore {
    pub nick_name: String,
    pub address: String,
    pub mnemonic: String,
    pub private_key: String,
}

/// Options for JS-compatible keystore encryption.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct KeystoreEncryptOptions {
    pub cipher: String,
    pub dklen: usize,
    pub n: u32,
    pub r: u32,
    pub p: u32,
    pub salt: Option<Vec<u8>>,
    pub iv: Option<Vec<u8>>,
    pub nick_name: Option<String>,
    pub address: Option<String>,
}

impl Default for KeystoreEncryptOptions {
    fn default() -> Self {
        Self {
            cipher: DEFAULT_CIPHER.to_owned(),
            dklen: DEFAULT_DKLEN,
            n: DEFAULT_N,
            r: DEFAULT_R,
            p: DEFAULT_P,
            salt: None,
            iv: None,
            nick_name: None,
            address: None,
        }
    }
}

impl Keystore {
    /// Encrypts a wallet with the default JS-compatible keystore parameters.
    pub fn encrypt_js(wallet: &Wallet, password: &str) -> Result<Self, KeystoreError> {
        Self::encrypt_js_with_options(wallet, password, KeystoreEncryptOptions::default())
    }

    /// Encrypts a wallet with explicit keystore options.
    pub fn encrypt_js_with_options(
        wallet: &Wallet,
        password: &str,
        options: KeystoreEncryptOptions,
    ) -> Result<Self, KeystoreError> {
        let spec = CipherSpec::parse(&options.cipher)?;
        ensure_derived_key_len(options.dklen, spec)?;
        let salt = options.salt.unwrap_or_else(|| random_bytes(32));
        let iv = options.iv.unwrap_or_else(|| random_bytes(spec.iv_len()));
        let derived_key = Zeroizing::new(derive_key(
            password,
            &salt,
            options.n,
            options.r,
            options.p,
            options.dklen,
        )?);
        let private_key = Zeroizing::new(hex::decode(wallet.private_key())?);
        let private_key_ciphertext =
            encrypt(spec, &derived_key[..spec.key_len()], &iv, &private_key)?;
        let mnemonic_ciphertext = encrypt(
            spec,
            &derived_key[..spec.key_len()],
            &iv,
            wallet.mnemonic().as_bytes(),
        )?;

        let mut raw_mac = Zeroizing::new(derived_key[16..].to_vec());
        raw_mac.extend_from_slice(&private_key_ciphertext);

        Ok(Self {
            version: 1,
            kind: "aelf".to_owned(),
            nick_name: options.nick_name.unwrap_or_default(),
            id: None,
            address: options
                .address
                .unwrap_or_else(|| wallet.address().to_owned()),
            crypto: KeystoreCrypto {
                cipher: spec.as_str().to_owned(),
                ciphertext: hex::encode(private_key_ciphertext),
                cipherparams: CipherParams {
                    iv: hex::encode(iv),
                },
                mnemonic_encrypted: hex::encode(mnemonic_ciphertext),
                kdf: "scrypt".to_owned(),
                kdfparams: KdfParams {
                    r: options.r,
                    n: options.n,
                    p: options.p,
                    dklen: options.dklen,
                    salt: hex::encode(salt),
                },
                mac: keccak256_hex(&raw_mac),
            },
        })
    }

    /// Decrypts the keystore and returns the recovered wallet data.
    pub fn unlock_js(&self, password: &str) -> Result<UnlockedKeystore, KeystoreError> {
        let spec = CipherSpec::parse(&self.crypto.cipher)?;
        ensure_derived_key_len(self.crypto.kdfparams.dklen, spec)?;
        let salt = hex::decode(&self.crypto.kdfparams.salt)?;
        let iv = hex::decode(&self.crypto.cipherparams.iv)?;
        let ciphertext = hex::decode(&self.crypto.ciphertext)?;
        let mnemonic_ciphertext = hex::decode(&self.crypto.mnemonic_encrypted)?;
        let derived_key = Zeroizing::new(derive_key(
            password,
            &salt,
            self.crypto.kdfparams.n,
            self.crypto.kdfparams.r,
            self.crypto.kdfparams.p,
            self.crypto.kdfparams.dklen,
        )?);

        let mut raw_mac = Zeroizing::new(derived_key[16..].to_vec());
        raw_mac.extend_from_slice(&ciphertext);
        if keccak256_hex(&raw_mac) != self.crypto.mac {
            return Err(KeystoreError::InvalidPassword);
        }

        let private_key = Zeroizing::new(decrypt(
            spec,
            &derived_key[..spec.key_len()],
            &iv,
            &ciphertext,
        )?);
        let mnemonic = Zeroizing::new(decrypt(
            spec,
            &derived_key[..spec.key_len()],
            &iv,
            &mnemonic_ciphertext,
        )?);

        Ok(UnlockedKeystore {
            nick_name: self.nick_name.clone(),
            address: self.address.clone(),
            mnemonic: String::from_utf8_lossy(&mnemonic).into_owned(),
            private_key: hex::encode(&*private_key),
        })
    }

    /// Checks whether the provided password can unlock the keystore.
    pub fn check_password(&self, password: &str) -> bool {
        self.unlock_js(password).is_ok()
    }
}

impl fmt::Debug for UnlockedKeystore {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("UnlockedKeystore")
            .field("nick_name", &self.nick_name)
            .field("address", &self.address)
            .field("mnemonic", &"<redacted>")
            .field("private_key", &"<redacted>")
            .finish()
    }
}

impl Drop for UnlockedKeystore {
    fn drop(&mut self) {
        self.mnemonic.zeroize();
        self.private_key.zeroize();
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum CipherSpec {
    Aes128Ctr,
    Aes192Ctr,
    Aes256Ctr,
    Aes128Cbc,
    Aes192Cbc,
    Aes256Cbc,
}

impl CipherSpec {
    fn parse(value: &str) -> Result<Self, KeystoreError> {
        match value.to_ascii_lowercase().as_str() {
            "aes-128-ctr" => Ok(Self::Aes128Ctr),
            "aes-192-ctr" => Ok(Self::Aes192Ctr),
            "aes-256-ctr" => Ok(Self::Aes256Ctr),
            "aes-128-cbc" | "aes128" => Ok(Self::Aes128Cbc),
            "aes-192-cbc" | "aes192" => Ok(Self::Aes192Cbc),
            "aes-256-cbc" | "aes256" => Ok(Self::Aes256Cbc),
            other => Err(KeystoreError::UnsupportedCipher(other.to_owned())),
        }
    }

    fn as_str(self) -> &'static str {
        match self {
            Self::Aes128Ctr => "aes-128-ctr",
            Self::Aes192Ctr => "aes-192-ctr",
            Self::Aes256Ctr => "aes-256-ctr",
            Self::Aes128Cbc => "aes-128-cbc",
            Self::Aes192Cbc => "aes-192-cbc",
            Self::Aes256Cbc => "aes-256-cbc",
        }
    }

    fn key_len(self) -> usize {
        match self {
            Self::Aes128Ctr | Self::Aes128Cbc => 16,
            Self::Aes192Ctr | Self::Aes192Cbc => 24,
            Self::Aes256Ctr | Self::Aes256Cbc => 32,
        }
    }

    fn iv_len(self) -> usize {
        16
    }
}

fn derive_key(
    password: &str,
    salt: &[u8],
    n: u32,
    r: u32,
    p: u32,
    dklen: usize,
) -> Result<Vec<u8>, KeystoreError> {
    if !n.is_power_of_two() || n == 0 || r == 0 || p == 0 || dklen == 0 {
        return Err(KeystoreError::InvalidScryptParams);
    }
    let log_n = n.ilog2() as u8;
    let params = match Params::new(log_n, r, p, dklen) {
        Ok(params) => params,
        Err(_) => return derive_key_compat(password, salt, n, r, p, dklen),
    };
    let mut derived_key = vec![0_u8; dklen];
    scrypt(password.as_bytes(), salt, &params, &mut derived_key)
        .map_err(|_| KeystoreError::InvalidScryptParams)?;
    Ok(derived_key)
}

// Legacy Nethereum/C# keystores can use parameter sets rejected by RustCrypto's stricter guard.
fn derive_key_compat(
    password: &str,
    salt: &[u8],
    n: u32,
    r: u32,
    p: u32,
    dklen: usize,
) -> Result<Vec<u8>, KeystoreError> {
    let params = CompatScryptParams::new(n, r, p, dklen)?;
    compat_scrypt(password.as_bytes(), salt, &params)
}

fn ensure_derived_key_len(dklen: usize, spec: CipherSpec) -> Result<(), KeystoreError> {
    let minimum = cmp::max(16, spec.key_len());
    if dklen < minimum {
        return Err(KeystoreError::InvalidScryptParams);
    }
    Ok(())
}

fn encrypt(
    spec: CipherSpec,
    key: &[u8],
    iv: &[u8],
    plaintext: &[u8],
) -> Result<Vec<u8>, KeystoreError> {
    match spec {
        CipherSpec::Aes128Ctr => encrypt_aes128_ctr(key, iv, plaintext),
        CipherSpec::Aes192Ctr => encrypt_aes192_ctr(key, iv, plaintext),
        CipherSpec::Aes256Ctr => encrypt_aes256_ctr(key, iv, plaintext),
        CipherSpec::Aes128Cbc => encrypt_aes128_cbc(key, iv, plaintext),
        CipherSpec::Aes192Cbc => encrypt_aes192_cbc(key, iv, plaintext),
        CipherSpec::Aes256Cbc => encrypt_aes256_cbc(key, iv, plaintext),
    }
}

fn decrypt(
    spec: CipherSpec,
    key: &[u8],
    iv: &[u8],
    ciphertext: &[u8],
) -> Result<Vec<u8>, KeystoreError> {
    match spec {
        CipherSpec::Aes128Ctr => decrypt_aes128_ctr(key, iv, ciphertext),
        CipherSpec::Aes192Ctr => decrypt_aes192_ctr(key, iv, ciphertext),
        CipherSpec::Aes256Ctr => decrypt_aes256_ctr(key, iv, ciphertext),
        CipherSpec::Aes128Cbc => decrypt_aes128_cbc(key, iv, ciphertext),
        CipherSpec::Aes192Cbc => decrypt_aes192_cbc(key, iv, ciphertext),
        CipherSpec::Aes256Cbc => decrypt_aes256_cbc(key, iv, ciphertext),
    }
}

fn encrypt_aes128_ctr(key: &[u8], iv: &[u8], plaintext: &[u8]) -> Result<Vec<u8>, KeystoreError> {
    encrypt_ctr::<Ctr128BE<Aes128>>(key, iv, plaintext)
}

fn decrypt_aes128_ctr(key: &[u8], iv: &[u8], ciphertext: &[u8]) -> Result<Vec<u8>, KeystoreError> {
    decrypt_ctr::<Ctr128BE<Aes128>>(key, iv, ciphertext)
}

fn encrypt_aes192_ctr(key: &[u8], iv: &[u8], plaintext: &[u8]) -> Result<Vec<u8>, KeystoreError> {
    encrypt_ctr::<Ctr128BE<Aes192>>(key, iv, plaintext)
}

fn decrypt_aes192_ctr(key: &[u8], iv: &[u8], ciphertext: &[u8]) -> Result<Vec<u8>, KeystoreError> {
    decrypt_ctr::<Ctr128BE<Aes192>>(key, iv, ciphertext)
}

fn encrypt_aes256_ctr(key: &[u8], iv: &[u8], plaintext: &[u8]) -> Result<Vec<u8>, KeystoreError> {
    encrypt_ctr::<Ctr128BE<Aes256>>(key, iv, plaintext)
}

fn decrypt_aes256_ctr(key: &[u8], iv: &[u8], ciphertext: &[u8]) -> Result<Vec<u8>, KeystoreError> {
    decrypt_ctr::<Ctr128BE<Aes256>>(key, iv, ciphertext)
}

fn encrypt_aes128_cbc(key: &[u8], iv: &[u8], plaintext: &[u8]) -> Result<Vec<u8>, KeystoreError> {
    encrypt_cbc::<Aes128>(key, iv, plaintext)
}

fn decrypt_aes128_cbc(key: &[u8], iv: &[u8], ciphertext: &[u8]) -> Result<Vec<u8>, KeystoreError> {
    decrypt_cbc::<Aes128>(key, iv, ciphertext)
}

fn encrypt_aes192_cbc(key: &[u8], iv: &[u8], plaintext: &[u8]) -> Result<Vec<u8>, KeystoreError> {
    encrypt_cbc::<Aes192>(key, iv, plaintext)
}

fn decrypt_aes192_cbc(key: &[u8], iv: &[u8], ciphertext: &[u8]) -> Result<Vec<u8>, KeystoreError> {
    decrypt_cbc::<Aes192>(key, iv, ciphertext)
}

fn encrypt_aes256_cbc(key: &[u8], iv: &[u8], plaintext: &[u8]) -> Result<Vec<u8>, KeystoreError> {
    encrypt_cbc::<Aes256>(key, iv, plaintext)
}

fn decrypt_aes256_cbc(key: &[u8], iv: &[u8], ciphertext: &[u8]) -> Result<Vec<u8>, KeystoreError> {
    decrypt_cbc::<Aes256>(key, iv, ciphertext)
}

fn encrypt_ctr<C>(key: &[u8], iv: &[u8], plaintext: &[u8]) -> Result<Vec<u8>, KeystoreError>
where
    C: KeyIvInit + StreamCipher,
{
    let mut cipher = C::new_from_slices(key, iv)?;
    let mut output = plaintext.to_vec();
    cipher.apply_keystream(&mut output);
    Ok(output)
}

fn decrypt_ctr<C>(key: &[u8], iv: &[u8], ciphertext: &[u8]) -> Result<Vec<u8>, KeystoreError>
where
    C: KeyIvInit + StreamCipher,
{
    encrypt_ctr::<C>(key, iv, ciphertext)
}

fn encrypt_cbc<C>(key: &[u8], iv: &[u8], plaintext: &[u8]) -> Result<Vec<u8>, KeystoreError>
where
    C: cipher::BlockCipher + cipher::BlockEncryptMut + cipher::KeyInit,
{
    let mut buffer = vec![0_u8; plaintext.len() + 16];
    buffer[..plaintext.len()].copy_from_slice(plaintext);
    let encrypted = CbcEncryptor::<C>::new_from_slices(key, iv)?
        .encrypt_padded_mut::<Pkcs7>(&mut buffer, plaintext.len())
        .map_err(|_| KeystoreError::CipherPadding)?;
    Ok(encrypted.to_vec())
}

fn decrypt_cbc<C>(key: &[u8], iv: &[u8], ciphertext: &[u8]) -> Result<Vec<u8>, KeystoreError>
where
    C: cipher::BlockCipher + cipher::BlockDecryptMut + cipher::KeyInit,
{
    let mut buffer = ciphertext.to_vec();
    let decrypted = CbcDecryptor::<C>::new_from_slices(key, iv)?
        .decrypt_padded_mut::<Pkcs7>(&mut buffer)
        .map_err(|_| KeystoreError::InvalidPassword)?;
    Ok(decrypted.to_vec())
}

fn random_bytes(len: usize) -> Vec<u8> {
    let mut bytes = vec![0_u8; len];
    rng().fill_bytes(&mut bytes);
    bytes
}

fn keccak256_hex(bytes: &[u8]) -> String {
    hex::encode(Keccak256::digest(bytes))
}

#[derive(Clone, Copy, Debug)]
struct CompatScryptParams {
    n: usize,
    r: usize,
    p: usize,
    dklen: usize,
}

impl CompatScryptParams {
    fn new(n: u32, r: u32, p: u32, dklen: usize) -> Result<Self, KeystoreError> {
        if !n.is_power_of_two() || n == 0 || r == 0 || p == 0 || dklen == 0 {
            return Err(KeystoreError::InvalidScryptParams);
        }
        if dklen / 32 > 0xffff_ffff {
            return Err(KeystoreError::InvalidScryptParams);
        }

        let n = n as usize;
        let r = r as usize;
        let p = p as usize;

        let r128 = r
            .checked_mul(128)
            .ok_or(KeystoreError::InvalidScryptParams)?;
        let pr128 = p
            .checked_mul(r128)
            .ok_or(KeystoreError::InvalidScryptParams)?;
        let nr128 = n
            .checked_mul(r128)
            .ok_or(KeystoreError::InvalidScryptParams)?;

        // Keep the compatibility branch bounded and allocation-safe.
        let _ = pr128
            .checked_add(nr128)
            .ok_or(KeystoreError::InvalidScryptParams)?;

        Ok(Self { n, r, p, dklen })
    }
}

fn compat_scrypt(
    password: &[u8],
    salt: &[u8],
    params: &CompatScryptParams,
) -> Result<Vec<u8>, KeystoreError> {
    let r128 = params
        .r
        .checked_mul(128)
        .ok_or(KeystoreError::InvalidScryptParams)?;
    let pr128 = params
        .p
        .checked_mul(r128)
        .ok_or(KeystoreError::InvalidScryptParams)?;
    let nr128 = params
        .n
        .checked_mul(r128)
        .ok_or(KeystoreError::InvalidScryptParams)?;

    let mut b = Zeroizing::new(vec![0_u8; pr128]);
    pbkdf2_hmac::<sha2::Sha256>(password, salt, 1, &mut b);

    let mut v = Zeroizing::new(vec![0_u8; nr128]);
    let mut t = Zeroizing::new(vec![0_u8; r128]);

    for chunk in b.chunks_mut(r128) {
        compat_scrypt_ro_mix(chunk, &mut v, &mut t, params.n);
    }

    let mut derived_key = vec![0_u8; params.dklen];
    pbkdf2_hmac::<sha2::Sha256>(password, &b, 1, &mut derived_key);
    Ok(derived_key)
}

fn compat_scrypt_ro_mix(b: &mut [u8], v: &mut [u8], t: &mut [u8], n: usize) {
    let len = b.len();

    for chunk in v.chunks_mut(len) {
        chunk.copy_from_slice(b);
        compat_scrypt_block_mix(chunk, b);
    }

    for _ in 0..n {
        let j = compat_integerify(b, n);
        compat_xor(b, &v[j * len..(j + 1) * len], t);
        compat_scrypt_block_mix(t, b);
    }
}

fn compat_integerify(x: &[u8], n: usize) -> usize {
    let mask = n - 1;
    let t = u32::from_le_bytes(
        x[x.len() - 64..x.len() - 60]
            .try_into()
            .expect("integerify slice"),
    );
    (t as usize) & mask
}

fn compat_scrypt_block_mix(input: &[u8], output: &mut [u8]) {
    type Salsa20_8 = SalsaCore<U4>;

    let mut x = [0_u8; 64];
    x.copy_from_slice(&input[input.len() - 64..]);

    let mut t = [0_u8; 64];

    for (i, chunk) in input.chunks(64).enumerate() {
        compat_xor(&x, chunk, &mut t);

        let mut state = [0_u32; 16];
        for (chunk, word) in t.chunks_exact(4).zip(state.iter_mut()) {
            *word = u32::from_le_bytes(chunk.try_into().expect("salsa chunk"));
        }

        Salsa20_8::from_raw_state(state).write_keystream_block((&mut x).into());

        let pos = if i % 2 == 0 {
            (i / 2) * 64
        } else {
            (i / 2) * 64 + input.len() / 2
        };

        output[pos..pos + 64].copy_from_slice(&x);
    }
}

fn compat_xor(x: &[u8], y: &[u8], output: &mut [u8]) {
    for ((out, &lhs), &rhs) in output.iter_mut().zip(x.iter()).zip(y.iter()) {
        *out = lhs ^ rhs;
    }
}

#[cfg(test)]
mod tests {
    use super::{Keystore, KeystoreEncryptOptions};
    use aelf_crypto::Wallet;

    const CSHARP_KEYSTORE_FIXTURE: &str = include_str!(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../tests/fixtures/csharp-keystore-v3.json"
    ));
    const TEST_MNEMONIC: &str =
        "orange learn result add snack curtain double state expose bless also clarify";
    const TEST_PRIVATE_KEY: &str =
        "ff96c3463af0b8629f170f078f97ac0147490b92e1784e3bff93f7ee9d1abcb6";

    #[test]
    fn roundtrip_default_keystore() {
        let wallet = Wallet::from_mnemonic(TEST_MNEMONIC).expect("wallet");
        let keystore = Keystore::encrypt_js(&wallet, "123123").expect("keystore");
        let unlocked = keystore.unlock_js("123123").expect("unlock");
        assert_eq!(unlocked.private_key, wallet.private_key());
        assert_eq!(unlocked.mnemonic, wallet.mnemonic());
        assert_eq!(unlocked.address, wallet.address());
    }

    #[test]
    fn supports_dk_len_alias_on_import() {
        let wallet = Wallet::from_mnemonic(TEST_MNEMONIC).expect("wallet");
        let keystore = Keystore::encrypt_js_with_options(
            &wallet,
            "123123",
            KeystoreEncryptOptions {
                salt: Some(vec![0x11; 32]),
                iv: Some(vec![0x22; 16]),
                ..KeystoreEncryptOptions::default()
            },
        )
        .expect("keystore");
        let mut json = serde_json::to_value(&keystore).expect("json");
        let dklen = json["crypto"]["kdfparams"]["dklen"]
            .as_u64()
            .expect("dklen");
        json["crypto"]["kdfparams"]
            .as_object_mut()
            .expect("kdfparams")
            .insert("dkLen".to_owned(), serde_json::json!(dklen));
        json["crypto"]["kdfparams"]
            .as_object_mut()
            .expect("kdfparams")
            .remove("dklen");
        let imported: Keystore = serde_json::from_value(json).expect("import");
        let unlocked = imported.unlock_js("123123").expect("unlock");
        assert_eq!(unlocked.private_key, wallet.private_key());
    }

    #[test]
    fn wrong_password_returns_false() {
        let wallet = Wallet::from_mnemonic(TEST_MNEMONIC).expect("wallet");
        let keystore = Keystore::encrypt_js(&wallet, "123123").expect("keystore");
        assert!(!keystore.check_password("wrong-password"));
    }

    #[test]
    fn rejects_short_dklen_before_slice() {
        let wallet = Wallet::from_mnemonic(TEST_MNEMONIC).expect("wallet");
        let error = Keystore::encrypt_js_with_options(
            &wallet,
            "123123",
            KeystoreEncryptOptions {
                dklen: 8,
                ..KeystoreEncryptOptions::default()
            },
        )
        .expect_err("short dklen should fail");
        assert!(matches!(error, super::KeystoreError::InvalidScryptParams));
    }

    #[test]
    fn imports_csharp_v3_keystore_fixture() {
        let keystore: Keystore = serde_json::from_str(CSHARP_KEYSTORE_FIXTURE).expect("fixture");
        let unlocked = keystore.unlock_js("abcde").expect("unlock");
        assert_eq!(unlocked.private_key, TEST_PRIVATE_KEY);
        assert_eq!(unlocked.mnemonic, "");
        assert_eq!(
            unlocked.address,
            "VQFq9atg4fMtFLhqpVh48ZnhX8FXMGBHW8MDANPpCSHcZisU6"
        );
    }

    #[test]
    fn debug_redacts_unlocked_keystore_secrets() {
        let wallet = Wallet::from_mnemonic(TEST_MNEMONIC).expect("wallet");
        let keystore = Keystore::encrypt_js(&wallet, "123123").expect("keystore");
        let unlocked = keystore.unlock_js("123123").expect("unlock");
        let debug = format!("{unlocked:?}");
        assert!(!debug.contains(TEST_MNEMONIC));
        assert!(!debug.contains(wallet.private_key()));
        assert!(debug.contains(wallet.address()));
    }

    #[test]
    fn compat_scrypt_matches_standard_scrypt_for_supported_params() {
        let salt = [0x11_u8; 32];
        let standard = super::derive_key("123123", &salt, 8192, 8, 1, 32).expect("standard");
        let compat = super::derive_key_compat("123123", &salt, 8192, 8, 1, 32).expect("compat");
        assert_eq!(compat, standard);
    }
}
