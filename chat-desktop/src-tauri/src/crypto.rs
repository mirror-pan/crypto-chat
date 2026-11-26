//! Off-chain cryptography for the crypto-chat protocol.
//!
//! This module implements real end-to-end encryption:
//!   1. ECDH (Elliptic Curve Diffie-Hellman) over secp256k1 — the same curve Ethereum uses.
//!   2. SHA-256 to turn the 32-byte ECDH shared secret into a symmetric AES-256 key.
//!   3. AES-256-GCM (AEAD) to encrypt the plaintext with a random 12-byte nonce.
//!
//! Security property: only the holder of the matching private key can recompute
//! the same shared secret and decrypt the ciphertext. The blockchain only ever
//! sees the resulting ciphertext (the `bytes` payload in the event log).
//!
//! Wire format stored on-chain:
//!   [ 0 .. 12 ]  IV / nonce (random per message)
//!   [ 12 ..   ]  AES-256-GCM ciphertext, with the 16-byte auth tag appended

use aes_gcm::{
    aead::{Aead, KeyInit},
    Aes256Gcm, Nonce,
};
use rand::{rngs::OsRng, RngCore};
use secp256k1::{ecdh::SharedSecret, PublicKey, Secp256k1, SecretKey};
use sha2::{Digest, Sha256};

/// Length of the AES-GCM nonce (IV) in bytes, prepended to every payload.
pub const IV_LEN: usize = 12;

/// Strip an optional `0x` prefix from a hex string.
fn clean_hex(hex_str: &str) -> &str {
    hex_str.strip_prefix("0x").unwrap_or(hex_str)
}

/// Parse a hex-encoded secp256k1 private key (32 bytes).
fn parse_secret_key(private_key_hex: &str) -> Result<SecretKey, String> {
    let bytes = hex::decode(clean_hex(private_key_hex))
        .map_err(|e| format!("invalid private key hex: {}", e))?;
    SecretKey::from_slice(&bytes).map_err(|e| format!("invalid secp256k1 secret key: {}", e))
}

/// Parse a hex-encoded secp256k1 public key.
///
/// Three encodings are accepted:
///   - compressed: 33 bytes, `02`/`03` prefix
///   - uncompressed: 65 bytes, `04` prefix (what `public_key_from_private` emits)
///   - raw X||Y coordinates: 64 bytes, no prefix (what `cast wallet new --json` emits
///     in its `public_key` field). We normalise it by prepending `04`.
fn parse_public_key(public_key_hex: &str) -> Result<PublicKey, String> {
    let mut bytes = hex::decode(clean_hex(public_key_hex))
        .map_err(|e| format!("invalid public key hex: {}", e))?;

    if bytes.len() == 64 {
        bytes.insert(0, 0x04);
    }

    PublicKey::from_slice(&bytes).map_err(|e| format!("invalid secp256k1 public key: {}", e))
}

/// Derive the uncompressed public key (65 bytes, `04` prefix) from a private key.
///
/// Exchange this value with your peer out-of-band (e.g. in person). The Ethereum
/// *address* is NOT enough — it is a one-way hash of the public key and cannot
/// be used to perform ECDH.
pub fn public_key_from_private(private_key_hex: &str) -> Result<String, String> {
    let secp = Secp256k1::new();
    let secret = parse_secret_key(private_key_hex)?;
    let public = PublicKey::from_secret_key(&secp, &secret);
    Ok(hex::encode(public.serialize_uncompressed()))
}

/// Derive the 32-byte symmetric AES key via ECDH.
///
/// `my_private_key` is our own secret; `peer_public_key` is the other party's
/// public key. Due to the commutativity of elliptic curve scalar multiplication,
/// `my_priv * peer_pub == peer_priv * my_pub`, so both sides derive the same key
/// without ever transmitting it.
fn derive_shared_key(my_private_key: &str, peer_public_key: &str) -> Result<[u8; 32], String> {
    let secret = parse_secret_key(my_private_key)?;
    let public = parse_public_key(peer_public_key)?;

    let shared = SharedSecret::new(&public, &secret);
    let shared_bytes = shared.secret_bytes();

    let aes_key = Sha256::digest(shared_bytes);
    let mut key = [0u8; 32];
    key.copy_from_slice(&aes_key);
    Ok(key)
}

/// Encrypt a plaintext message for a recipient.
///
/// Returns the on-chain payload: 12-byte IV followed by ciphertext + auth tag.
pub fn encrypt_message(
    my_private_key: &str,
    recipient_public_key: &str,
    plaintext: &str,
) -> Result<Vec<u8>, String> {
    let key = derive_shared_key(my_private_key, recipient_public_key)?;
    let cipher = Aes256Gcm::new_from_slice(&key)
        .map_err(|e| format!("failed to build AES-256-GCM cipher: {}", e))?;

    let mut iv = [0u8; IV_LEN];
    OsRng.fill_bytes(&mut iv);
    let nonce = Nonce::from_slice(&iv);

    let ciphertext = cipher
        .encrypt(nonce, plaintext.as_bytes())
        .map_err(|e| format!("AES-256-GCM encryption failed: {}", e))?;

    let mut payload = Vec::with_capacity(IV_LEN + ciphertext.len());
    payload.extend_from_slice(&iv);
    payload.extend_from_slice(&ciphertext);
    Ok(payload)
}

/// Decrypt an on-chain payload received from a sender.
///
/// `my_private_key` is our own secret; `sender_public_key` is the sender's
/// public key. The ECDH result matches the sender's derivation.
pub fn decrypt_message(
    my_private_key: &str,
    sender_public_key: &str,
    payload: &[u8],
) -> Result<String, String> {
    if payload.len() <= IV_LEN {
        return Err(format!(
            "payload too short: expected more than {} bytes, got {}",
            IV_LEN,
            payload.len()
        ));
    }

    let (iv, ciphertext) = payload.split_at(IV_LEN);
    let nonce = Nonce::from_slice(iv);

    let key = derive_shared_key(my_private_key, sender_public_key)?;
    let cipher = Aes256Gcm::new_from_slice(&key)
        .map_err(|e| format!("failed to build AES-256-GCM cipher: {}", e))?;

    let plaintext = cipher
        .decrypt(nonce, ciphertext)
        .map_err(|_| "decryption failed: wrong key or tampered ciphertext".to_string())?;

    String::from_utf8(plaintext).map_err(|e| format!("plaintext is not valid UTF-8: {}", e))
}

#[cfg(test)]
mod tests {
    use super::*;

    // Deterministic Anvil test keys. Never use these outside local testing.
    const ALICE_SK: &str =
        "0xac0974bec39a17e36ba4a6b4d238ff944bacb478cbed5efcae784d7bf4f2ff80";
    const BOB_SK: &str = "0x59c6995e998f97a5a0044966f0945389dc9e86dae88c7a8412f4603b6b78690d";

    #[test]
    fn ecdh_roundtrip_succeeds() {
        let alice_pub = public_key_from_private(ALICE_SK).unwrap();
        let bob_pub = public_key_from_private(BOB_SK).unwrap();

        let payload = encrypt_message(ALICE_SK, &bob_pub, "hello from alice").unwrap();
        let decrypted = decrypt_message(BOB_SK, &alice_pub, &payload).unwrap();

        assert_eq!(decrypted, "hello from alice");
    }

    #[test]
    fn raw_xy_public_key_from_cast_is_accepted() {
        // `cast wallet new --json` emits 64-byte X||Y without the 04 prefix.
        let uncompressed = public_key_from_private(BOB_SK).unwrap();
        let raw_xy = uncompressed.trim_start_matches("04");

        let payload = encrypt_message(ALICE_SK, raw_xy, "hello raw xy").unwrap();
        let alice_pub = public_key_from_private(ALICE_SK).unwrap();
        let decrypted = decrypt_message(BOB_SK, &alice_pub, &payload).unwrap();

        assert_eq!(decrypted, "hello raw xy");
    }

    #[test]
    fn wrong_recipient_key_fails_to_decrypt() {
        let bob_pub = public_key_from_private(BOB_SK).unwrap();
        let payload = encrypt_message(ALICE_SK, &bob_pub, "secret").unwrap();

        // Charlie cannot decrypt a message addressed to Bob.
        let charlie_sk =
            "0x5de4111afa1a4b94908f83103eb1f1706367c2e68ca870fc3fb9a804cdab365a";
        let alice_pub = public_key_from_private(ALICE_SK).unwrap();
        assert!(decrypt_message(charlie_sk, &alice_pub, &payload).is_err());
    }
}
