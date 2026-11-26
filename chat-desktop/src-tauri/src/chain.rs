//! On-chain relay for encrypted chat messages.
//!
//! This module owns everything that talks to Ethereum. It is called only from
//! Rust Tauri commands — the React UI never sees the private key or the RPC
//! internals. The chain only ever receives ciphertext (the `bytes` payload).

use ethers::prelude::*;
use ethers::signers::Signer;
use std::collections::HashSet;
use std::env;
use std::sync::Arc;

// ABI generated from Chat.sol (Phase 1). Only the pieces we use are declared.
abigen!(
    ChatContract,
    r#"[
        {
            "type": "function",
            "name": "sendMessage",
            "inputs": [
                { "name": "recipient", "type": "address", "internalType": "address" },
                { "name": "encryptedContent", "type": "bytes", "internalType": "bytes" }
            ],
            "outputs": [],
            "stateMutability": "nonpayable"
        },
        {
            "type": "event",
            "name": "MessageSent",
            "inputs": [
                { "name": "sender", "type": "address", "indexed": true, "internalType": "address" },
                { "name": "recipient", "type": "address", "indexed": true, "internalType": "address" },
                { "name": "encryptedContent", "type": "bytes", "indexed": false, "internalType": "bytes" },
                { "name": "timestamp", "type": "uint256", "indexed": false, "internalType": "uint256" }
            ],
            "anonymous": false
        },
        {
            "type": "function",
            "name": "registerPublicKey",
            "inputs": [
                { "name": "publicKey", "type": "bytes", "internalType": "bytes" }
            ],
            "outputs": [],
            "stateMutability": "nonpayable"
        },
        {
            "type": "function",
            "name": "publicKeys",
            "inputs": [
                { "name": "", "type": "address", "internalType": "address" }
            ],
            "outputs": [
                { "name": "", "type": "bytes", "internalType": "bytes" }
            ],
            "stateMutability": "view"
        },
        {
            "type": "event",
            "name": "PublicKeyRegistered",
            "inputs": [
                { "name": "user", "type": "address", "indexed": true, "internalType": "address" },
                { "name": "publicKey", "type": "bytes", "indexed": false, "internalType": "bytes" }
            ],
            "anonymous": false
        }
    ]"#
);

/// Default: Sepolia. Override with CHAT_RPC / CHAT_CONTRACT env vars if needed.
const DEFAULT_RPC: &str = "https://ethereum-sepolia-rpc.publicnode.com";
const DEFAULT_CONTRACT: &str = "0x3C639E282571A994536cC590D583C5EF9b0425B3";
/// Where to start scanning event logs. Set to the deployment block region to
/// avoid public RPC range limits. Override with CHAT_FROM_BLOCK if redeployed.
const DEFAULT_FROM_BLOCK: u64 = 12216488;

/// One decrypted message returned to the UI. All fields are public — the
/// private key never appears here.
#[derive(serde::Serialize)]
pub struct FetchedMessage {
    pub from: String,   // "me" or "them"
    pub text: String,
    pub tx_hash: String,
    pub timestamp: u64,
}

/// Public identity derived from a private key. Only address + public key are
/// returned to the UI; the private key itself never leaves Rust memory.
#[derive(serde::Serialize)]
pub struct Identity {
    pub address: String,
    pub pubkey: String,
}

/// Derive the public identity (address + uncompressed public key) from a key.
/// Used by the `unlock` command to validate and display who signed in.
pub fn identity_from_private(private_key: &str) -> Result<Identity, String> {
    let wallet: LocalWallet = private_key
        .parse()
        .map_err(|e| format!("invalid private key: {}", e))?;
    Ok(Identity {
        address: format!("{:?}", wallet.address()),
        pubkey: crate::crypto::public_key_from_private(private_key)?,
    })
}

/// Build a read-only provider and the configured contract address.
fn read_context() -> Result<(Provider<Http>, Address), String> {
    let rpc = env::var("CHAT_RPC").unwrap_or_else(|_| DEFAULT_RPC.to_string());
    let contract = env::var("CHAT_CONTRACT").unwrap_or_else(|_| DEFAULT_CONTRACT.to_string());
    let provider = Provider::<Http>::try_from(rpc.as_str())
        .map_err(|e| format!("invalid RPC URL: {}", e))?;
    let contract_address: Address = contract
        .parse()
        .map_err(|e| format!("invalid contract address: {}", e))?;
    Ok((provider, contract_address))
}

/// Outcome of ensuring our public key is published on-chain.
#[derive(serde::Serialize)]
pub struct Registration {
    /// "registered" when a transaction was sent, "already" when no op was needed.
    pub status: String,
    /// Transaction hash, present only when a transaction was sent.
    pub tx_hash: Option<String>,
}

/// Normalise a public key to the registry's canonical form: 64-byte X||Y hex,
/// without 0x or the 04 prefix. The contract stores exactly this form.
fn canonical_pubkey_hex(pubkey: &str) -> String {
    let mut hex = pubkey.trim_start_matches("0x");
    if hex.starts_with("04") {
        hex = &hex[2..];
    }
    hex.to_lowercase()
}

/// Publish our public key on-chain so peers can discover it by our address.
/// Idempotent: if the registry already holds our key, no transaction is sent.
/// This prevents a redundant gas spend and the "replacement transaction
/// underpriced" error when the same account logs in again.
pub async fn register_pubkey(private_key: &str) -> Result<Registration, String> {
    let identity = identity_from_private(private_key)?;
    let canonical = canonical_pubkey_hex(&identity.pubkey);

    // Fast path: key already published. Skip the transaction entirely.
    if let Ok(existing) = lookup_pubkey(&identity.address).await {
        if existing.eq_ignore_ascii_case(&canonical) {
            return Ok(Registration {
                status: "already".to_string(),
                tx_hash: None,
            });
        }
    }

    let rpc = env::var("CHAT_RPC").unwrap_or_else(|_| DEFAULT_RPC.to_string());
    let contract = env::var("CHAT_CONTRACT").unwrap_or_else(|_| DEFAULT_CONTRACT.to_string());

    let provider = Provider::<Http>::try_from(rpc.as_str())
        .map_err(|e| format!("invalid RPC URL: {}", e))?;
    let chain_id = provider
        .get_chainid()
        .await
        .map_err(|e| format!("cannot reach RPC node: {}", e))?
        .as_u64();
    let wallet: LocalWallet = private_key
        .parse::<LocalWallet>()
        .map_err(|e| format!("invalid private key: {}", e))?
        .with_chain_id(chain_id);
    let client = Arc::new(SignerMiddleware::new(provider, wallet));

    let contract_address: Address = contract
        .parse()
        .map_err(|e| format!("invalid contract address: {}", e))?;
    let chat = ChatContract::new(contract_address, client);

    // Contract stores canonical 64-byte X||Y; it also accepts 65-byte 0x04.
    let key_bytes = hex::decode(identity.pubkey.trim_start_matches("0x"))
        .map_err(|e| format!("invalid public key hex: {}", e))?;
    let call = chat.register_public_key(key_bytes.into());
    let pending = match call.send().await {
        Ok(pending) => pending,
        Err(e) => {
            // A concurrent login may have published the same key between our
            // lookup and send. Re-check before surfacing a nonce error.
            let message = e.to_string();
            if message.contains("replacement") || message.contains("nonce") {
                if let Ok(existing) = lookup_pubkey(&identity.address).await {
                    if existing.eq_ignore_ascii_case(&canonical) {
                        return Ok(Registration {
                            status: "already".to_string(),
                            tx_hash: None,
                        });
                    }
                }
            }
            return Err(format!("registration transaction failed: {}", message));
        }
    };
    Ok(Registration {
        status: "registered".to_string(),
        tx_hash: Some(format!("{:?}", pending.tx_hash())),
    })
}

/// Look up a peer's registered public key. Returns 64-byte hex without 0x04.
pub async fn lookup_pubkey(address: &str) -> Result<String, String> {
    let (provider, contract_address) = read_context()?;
    let target: Address = address
        .parse()
        .map_err(|e| format!("invalid address: {}", e))?;
    let chat = ChatContract::new(contract_address, Arc::new(provider));
    let key = chat
        .public_keys(target)
        .call()
        .await
        .map_err(|e| format!("failed to query public key registry: {}", e))?;
    if key.is_empty() {
        return Err("No public key registered for this address".to_string());
    }
    Ok(hex::encode(key))
}

/// A discovered contact: address plus the public key published on-chain.
#[derive(serde::Serialize, Clone)]
pub struct ContactInfo {
    pub address: String,
    pub pubkey: String,
}

/// Scan all MessageSent events for peers we have exchanged messages with, then
/// resolve each peer's public key from the on-chain registry. Contacts without
/// a registered key are skipped — they cannot be encrypted to yet.
pub async fn discover_contacts(private_key: &str) -> Result<Vec<ContactInfo>, String> {
    let wallet: LocalWallet = private_key
        .parse()
        .map_err(|e| format!("invalid private key: {}", e))?;
    let my_address = wallet.address();

    let (provider, contract_address) = read_context()?;
    let from_block: u64 = env::var("CHAT_FROM_BLOCK")
        .ok()
        .and_then(|s| s.parse().ok())
        .unwrap_or(DEFAULT_FROM_BLOCK);

    let filter = Filter::new()
        .address(contract_address)
        .event("MessageSent(address,address,bytes,uint256)")
        .from_block(from_block)
        .to_block(BlockNumber::Latest);
    let logs = provider
        .get_logs(&filter)
        .await
        .map_err(|e| format!("failed to query chain events: {}", e))?;

    let mut peers: HashSet<Address> = HashSet::new();
    for log in logs {
        if let Ok(event) =
            <MessageSentFilter as ethers::contract::EthEvent>::decode_log(&log.into())
        {
            if event.sender == my_address {
                peers.insert(event.recipient);
            } else if event.recipient == my_address {
                peers.insert(event.sender);
            }
        }
    }

    let chat = ChatContract::new(contract_address, Arc::new(provider));
    let mut result = Vec::new();
    for peer in peers {
        if let Ok(key) = chat.public_keys(peer).call().await {
            if !key.is_empty() {
                result.push(ContactInfo {
                    address: format!("{:?}", peer),
                    pubkey: format!("0x04{}", hex::encode(key)),
                });
            }
        }
    }
    Ok(result)
}

/// Encrypt `plaintext` for `peer_pubkey` and relay the ciphertext to Chat.sol.
/// Returns the transaction hash on success. `private_key` is passed in from the
/// caller (read from env) and is never returned or logged.
pub async fn send_encrypted(
    private_key: &str,
    recipient: &str,
    peer_pubkey: &str,
    plaintext: &str,
) -> Result<String, String> {
    // Step 1: encrypt off-chain. The chain never sees the plaintext.
    let payload = crate::crypto::encrypt_message(private_key, peer_pubkey, plaintext)?;

    // Step 2: connect to the node and build a signing client.
    let rpc = env::var("CHAT_RPC").unwrap_or_else(|_| DEFAULT_RPC.to_string());
    let contract = env::var("CHAT_CONTRACT").unwrap_or_else(|_| DEFAULT_CONTRACT.to_string());

    let provider = Provider::<Http>::try_from(rpc.as_str())
        .map_err(|e| format!("invalid RPC URL: {}", e))?;
    let chain_id = provider
        .get_chainid()
        .await
        .map_err(|e| format!("cannot reach RPC node: {}", e))?
        .as_u64();

    let wallet: LocalWallet = private_key
        .parse::<LocalWallet>()
        .map_err(|e| format!("invalid private key: {}", e))?
        .with_chain_id(chain_id);
    let client = Arc::new(SignerMiddleware::new(provider, wallet));

    let recipient_address: Address = recipient
        .parse()
        .map_err(|e| format!("invalid recipient address: {}", e))?;
    let contract_address: Address = contract
        .parse()
        .map_err(|e| format!("invalid contract address: {}", e))?;

    // Step 3: relay the ciphertext to the contract's event log.
    let chat = ChatContract::new(contract_address, client);
    let call = chat.send_message(recipient_address, payload.into());
    let pending_tx = call
        .send()
        .await
        .map_err(|e| format!("transaction failed: {}", e))?;

    Ok(format!("{:?}", pending_tx.tx_hash()))
}

/// Read all MessageSent events involving us and `peer_address`, decrypt each
/// payload locally with our private key + the peer's public key, and return the
/// plaintexts. This is how the UI restores history after a refresh: the real
/// data lives on-chain, not in browser memory.
pub async fn fetch_messages(
    private_key: &str,
    peer_address: &str,
    peer_pubkey: &str,
) -> Result<Vec<FetchedMessage>, String> {
    // Our own address, derived from the key. Never sent back to the UI.
    let wallet: LocalWallet = private_key
        .parse::<LocalWallet>()
        .map_err(|e| format!("invalid private key: {}", e))?;
    let my_address = wallet.address();

    let peer: Address = peer_address
        .parse()
        .map_err(|e| format!("invalid peer address: {}", e))?;

    let rpc = env::var("CHAT_RPC").unwrap_or_else(|_| DEFAULT_RPC.to_string());
    let contract = env::var("CHAT_CONTRACT").unwrap_or_else(|_| DEFAULT_CONTRACT.to_string());
    let from_block: u64 = env::var("CHAT_FROM_BLOCK")
        .ok()
        .and_then(|s| s.parse().ok())
        .unwrap_or(DEFAULT_FROM_BLOCK);

    let provider = Provider::<Http>::try_from(rpc.as_str())
        .map_err(|e| format!("invalid RPC URL: {}", e))?;
    let contract_address: Address = contract
        .parse()
        .map_err(|e| format!("invalid contract address: {}", e))?;

    // Build the log filter EXPLICITLY with `.address(...)`.
    // Public RPC providers (e.g. publicnode.com) reject eth_getLogs without an
    // explicit contract address: code -32701 "Please specify an address...".
    // `chat.event::<T>()` did not satisfy that requirement in practice.
    let filter = Filter::new()
        .address(contract_address)
        .event("MessageSent(address,address,bytes,uint256)")
        .from_block(from_block)
        .to_block(BlockNumber::Latest);

    let logs = provider
        .get_logs(&filter)
        .await
        .map_err(|e| format!("failed to query chain events: {}", e))?;

    let mut result = Vec::new();
    for log in logs {
        let tx_hash = format!("{:?}", log.transaction_hash.unwrap_or_default());
        // Decode the ABI-encoded log into the generated event struct.
        // `decode_log` takes &RawLog; convert from the provider's Log type.
        // Fully-qualified: `decode_log` exists on both EthEvent and EthLogDecode.
        let event = match <MessageSentFilter as ethers::contract::EthEvent>::decode_log(&log.into()) {
            Ok(e) => e,
            Err(_) => continue,
        };

        // Only keep messages between us and this peer (either direction).
        let involves_peer = (event.sender == my_address && event.recipient == peer)
            || (event.sender == peer && event.recipient == my_address);
        if !involves_peer {
            continue;
        }

        let payload = event.encrypted_content.to_vec();
        let text = match crate::crypto::decrypt_message(private_key, peer_pubkey, &payload) {
            Ok(t) => t,
            Err(_) => {
                // A message to this peer but encrypted with a different key — skip.
                continue;
            }
        };

        result.push(FetchedMessage {
            from: if event.sender == my_address { "me" } else { "them" }.to_string(),
            text,
            tx_hash,
            timestamp: event.timestamp.as_u64(),
        });
    }
    Ok(result)
}
