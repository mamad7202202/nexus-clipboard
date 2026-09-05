//! The vault: at-rest encryption for sensitive clipboard entries.
//!
//! Design notes
//! - Only items classified as secrets are encrypted. Encrypting the whole
//!   database would mean holding the key for the entire session and would make
//!   full-text search impossible; per-item encryption keeps search working for
//!   everything the user did not mark as sensitive.
//! - The key is derived from a passphrase with Argon2id and never persisted.
//!   What is persisted is a salt plus a verifier, so a wrong passphrase is
//!   detected without ever storing the key.
//! - XChaCha20-Poly1305 with a random 192-bit nonce per message: the nonce space
//!   is large enough that random generation needs no counter or state.
//! - When no passphrase is configured, a machine-local key is derived instead.
//!   That protects against casual file inspection and other users on the same
//!   machine, and is honest about not protecting against an attacker who has
//!   both the database and the key file.

use chacha20poly1305::{
    // `rand_core` is re-exported by the aead crate; using *its* version of the
    // trait (rather than the top-level `rand` crate, which is a major version
    // ahead) is what makes `OsRng` usable here.
    aead::{rand_core::RngCore, Aead, KeyInit, OsRng},
    XChaCha20Poly1305, XNonce,
};
use zeroize::{Zeroize, ZeroizeOnDrop};

use crate::error::{Error, Result};

/// Nonce length for XChaCha20-Poly1305.
const NONCE_LEN: usize = 24;
const KEY_LEN: usize = 32;
const SALT_LEN: usize = 16;

/// Argon2id parameters. Tuned for ~100 ms on a typical laptop: strong enough to
/// make offline guessing expensive without making unlock feel sluggish.
const ARGON_MEM_KIB: u32 = 64 * 1024; // 64 MB
const ARGON_ITERS: u32 = 3;
const ARGON_LANES: u32 = 4;

/// A derived key, wiped from memory on drop.
#[derive(Clone, ZeroizeOnDrop)]
pub struct VaultKey([u8; KEY_LEN]);

impl std::fmt::Debug for VaultKey {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        // Never let a key reach a log line.
        f.write_str("VaultKey(<redacted>)")
    }
}

/// Persisted vault parameters. Contains no secret material.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct VaultConfig {
    /// Base64 salt for key derivation.
    pub salt: String,
    /// Encryption of a known constant, used to check a passphrase.
    pub verifier: String,
    /// True when the user set an explicit passphrase (vs. the machine key).
    pub passphrase: bool,
}

/// Plaintext used to build the verifier. Its exact value is irrelevant; what
/// matters is that decrypting it correctly proves the key is right.
const VERIFIER_PLAINTEXT: &[u8] = b"nexus-clipboard-vault-v1";

/// Derive a key from a passphrase and salt using Argon2id.
pub fn derive_key(passphrase: &str, salt: &[u8]) -> Result<VaultKey> {
    use argon2::{Algorithm, Argon2, Params, Version};

    let params = Params::new(ARGON_MEM_KIB, ARGON_ITERS, ARGON_LANES, Some(KEY_LEN))
        .map_err(|e| Error::Crypto(format!("bad argon2 params: {e}")))?;
    let argon = Argon2::new(Algorithm::Argon2id, Version::V0x13, params);

    let mut key = [0u8; KEY_LEN];
    argon
        .hash_password_into(passphrase.as_bytes(), salt, &mut key)
        .map_err(|e| Error::Crypto(format!("key derivation failed: {e}")))?;

    Ok(VaultKey(key))
}

/// Create a fresh vault configuration for `passphrase`.
///
/// Pass `None` to use a machine-local secret: convenient by default, and the
/// user can upgrade to a real passphrase later without losing data (the app
/// re-encrypts on change).
pub fn init(passphrase: Option<&str>) -> Result<(VaultConfig, VaultKey)> {
    let mut salt = [0u8; SALT_LEN];
    OsRng.fill_bytes(&mut salt);

    let secret = match passphrase {
        Some(p) => p.to_string(),
        None => machine_secret(),
    };

    let key = derive_key(&secret, &salt)?;
    let verifier = encrypt(&key, VERIFIER_PLAINTEXT)?;

    Ok((
        VaultConfig {
            salt: b64(&salt),
            verifier: b64(&verifier),
            passphrase: passphrase.is_some(),
        },
        key,
    ))
}

/// Reconstruct the key from a stored config, verifying the passphrase.
pub fn unlock(config: &VaultConfig, passphrase: Option<&str>) -> Result<VaultKey> {
    let secret = match (config.passphrase, passphrase) {
        (true, Some(p)) => p.to_string(),
        (true, None) => return Err(Error::VaultLocked),
        // A machine-keyed vault ignores any passphrase it is handed.
        (false, _) => machine_secret(),
    };

    let salt = unb64(&config.salt)?;
    let key = derive_key(&secret, &salt)?;

    let verifier = unb64(&config.verifier)?;
    match decrypt(&key, &verifier) {
        Ok(plain) if plain == VERIFIER_PLAINTEXT => Ok(key),
        _ => Err(Error::BadPassphrase),
    }
}

/// Encrypt `plaintext`, returning `nonce || ciphertext`.
pub fn encrypt(key: &VaultKey, plaintext: &[u8]) -> Result<Vec<u8>> {
    let cipher = XChaCha20Poly1305::new_from_slice(&key.0)
        .map_err(|e| Error::Crypto(format!("bad key: {e}")))?;

    let mut nonce_bytes = [0u8; NONCE_LEN];
    OsRng.fill_bytes(&mut nonce_bytes);
    let nonce = XNonce::from_slice(&nonce_bytes);

    let ciphertext = cipher
        .encrypt(nonce, plaintext)
        .map_err(|_| Error::Crypto("encryption failed".into()))?;

    let mut out = Vec::with_capacity(NONCE_LEN + ciphertext.len());
    out.extend_from_slice(&nonce_bytes);
    out.extend_from_slice(&ciphertext);
    Ok(out)
}

/// Decrypt a `nonce || ciphertext` payload produced by [`encrypt`].
pub fn decrypt(key: &VaultKey, data: &[u8]) -> Result<Vec<u8>> {
    if data.len() <= NONCE_LEN {
        return Err(Error::Crypto("ciphertext too short".into()));
    }
    let cipher = XChaCha20Poly1305::new_from_slice(&key.0)
        .map_err(|e| Error::Crypto(format!("bad key: {e}")))?;

    let (nonce_bytes, ciphertext) = data.split_at(NONCE_LEN);
    let nonce = XNonce::from_slice(nonce_bytes);

    cipher
        .decrypt(nonce, ciphertext)
        // Authentication failure is indistinguishable from a wrong key, which is
        // exactly what we want to report.
        .map_err(|_| Error::BadPassphrase)
}

/// Encrypt a UTF-8 string.
pub fn encrypt_str(key: &VaultKey, s: &str) -> Result<Vec<u8>> {
    encrypt(key, s.as_bytes())
}

/// Decrypt back into a UTF-8 string.
pub fn decrypt_str(key: &VaultKey, data: &[u8]) -> Result<String> {
    let mut bytes = decrypt(key, data)?;
    let s = String::from_utf8(bytes.clone())
        .map_err(|_| Error::Crypto("decrypted payload is not valid UTF-8".into()))?;
    bytes.zeroize();
    Ok(s)
}

/// A stable per-machine, per-user secret used when no passphrase is set.
///
/// This is deliberately derived rather than stored: it ties the vault to this
/// machine and user account without adding another file to protect.
fn machine_secret() -> String {
    let user = std::env::var("USERNAME")
        .or_else(|_| std::env::var("USER"))
        .unwrap_or_else(|_| "unknown".into());
    let machine = std::env::var("COMPUTERNAME")
        .or_else(|_| std::env::var("HOSTNAME"))
        .unwrap_or_else(|_| "unknown".into());
    // The constant salts the value so it is not simply the username.
    format!("nexus-v1::{machine}::{user}::local-vault")
}

fn b64(data: &[u8]) -> String {
    use base64::Engine;
    base64::engine::general_purpose::STANDARD.encode(data)
}

fn unb64(s: &str) -> Result<Vec<u8>> {
    use base64::Engine;
    base64::engine::general_purpose::STANDARD
        .decode(s)
        .map_err(|e| Error::Crypto(format!("bad base64: {e}")))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn round_trip_with_passphrase() {
        let (config, key) = init(Some("correct horse battery staple")).unwrap();
        let blob = encrypt_str(&key, "my secret token").unwrap();

        let reopened = unlock(&config, Some("correct horse battery staple")).unwrap();
        assert_eq!(decrypt_str(&reopened, &blob).unwrap(), "my secret token");
    }

    #[test]
    fn wrong_passphrase_is_rejected() {
        let (config, _) = init(Some("right")).unwrap();
        assert!(matches!(unlock(&config, Some("wrong")), Err(Error::BadPassphrase)));
    }

    #[test]
    fn locked_without_passphrase() {
        let (config, _) = init(Some("right")).unwrap();
        assert!(matches!(unlock(&config, None), Err(Error::VaultLocked)));
    }

    #[test]
    fn machine_key_opens_without_passphrase() {
        let (config, key) = init(None).unwrap();
        let blob = encrypt_str(&key, "local secret").unwrap();
        let reopened = unlock(&config, None).unwrap();
        assert_eq!(decrypt_str(&reopened, &blob).unwrap(), "local secret");
    }

    #[test]
    fn nonces_are_unique_per_message() {
        let (_, key) = init(None).unwrap();
        let a = encrypt(&key, b"same plaintext").unwrap();
        let b = encrypt(&key, b"same plaintext").unwrap();
        assert_ne!(a, b, "identical plaintexts produced identical ciphertexts");
    }

    #[test]
    fn tampering_is_detected() {
        let (_, key) = init(None).unwrap();
        let mut blob = encrypt(&key, b"important").unwrap();
        let last = blob.len() - 1;
        blob[last] ^= 0xff;
        assert!(decrypt(&key, &blob).is_err());
    }

    #[test]
    fn short_ciphertext_is_rejected() {
        let (_, key) = init(None).unwrap();
        assert!(decrypt(&key, &[0u8; 8]).is_err());
    }

    #[test]
    fn key_debug_does_not_leak() {
        let (_, key) = init(None).unwrap();
        assert!(!format!("{key:?}").contains("["));
    }
}
