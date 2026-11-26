//! On-chain relay for encrypted chat messages.
//!
//! This module owns everything that talks to Ethereum. It is called only from
//! Rust Tauri commands — the React UI never sees the private key or the RPC
//! internals. The chain only ever receives ciphertext (the `bytes` payload).

use ethers::prelude::*;
use ethers::signers::Signer;
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
        }
    ]"#
);

/// Default: Sepolia. Override with CHAT_RPC / CHAT_CONTRACT env vars if needed.
const DEFAULT_RPC: &str = "https://ethereum-sepolia-rpc.publicnode.com";
const DEFAULT_CONTRACT: &str = "0x21433635Df276caA6df2cD9F2489321797506cb0";
/// Where to start scanning event logs. Set to the deployment block region to
/// avoid public RPC range limits. Override with CHAT_FROM_BLOCK if redeployed.
const DEFAULT_FROM_BLOCK: u64 = 12209000;

/// One decrypted message returned to the UI. All fields are public — the
/// private key never appears here.
#[derive(serde::Serialize)]
pub struct FetchedMessage {
    pub from: String,   // "me" or "them"
    pub text: String,
    pub tx_hash: String,
    pub timestamp: u64,
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
