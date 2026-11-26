mod chain;
mod crypto;

use std::env;

// ---------------------------------------------------------------------------
// Tauri commands are the ONLY bridge between the React UI and the Rust core.
// The UI can call these, but it can never read the private key directly.
// ---------------------------------------------------------------------------

/// Return our uncompressed public key so the UI can display it.
/// The private key is read from CHAT_PRIVATE_KEY (set in the terminal that
/// launched `npm run tauri dev`) and NEVER leaves this Rust process.
#[tauri::command]
fn get_pubkey() -> Result<String, String> {
    let private_key = env::var("CHAT_PRIVATE_KEY")
        .map_err(|_| "CHAT_PRIVATE_KEY is not set in this terminal".to_string())?;
    crypto::public_key_from_private(&private_key)
}

/// Encrypt a message locally and relay the ciphertext to Chat.sol on Sepolia.
///
/// The UI passes only: recipient address, peer public key, and plaintext.
/// The private key is read here from the environment and never sent to the UI.
#[tauri::command]
async fn send_message(recipient: String, peer_pubkey: String, message: String) -> Result<String, String> {
    let private_key = env::var("CHAT_PRIVATE_KEY")
        .map_err(|_| "CHAT_PRIVATE_KEY is not set in this terminal".to_string())?;
    chain::send_encrypted(&private_key, &recipient, &peer_pubkey, &message).await
}

/// Fetch and decrypt the conversation history with a peer from on-chain logs.
/// Called on app start and after every send, so a refresh restores real messages.
#[tauri::command]
async fn fetch_messages(peer_address: String, peer_pubkey: String) -> Result<Vec<chain::FetchedMessage>, String> {
    let private_key = env::var("CHAT_PRIVATE_KEY")
        .map_err(|_| "CHAT_PRIVATE_KEY is not set in this terminal".to_string())?;
    chain::fetch_messages(&private_key, &peer_address, &peer_pubkey).await
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_opener::init())
        .invoke_handler(tauri::generate_handler![get_pubkey, send_message, fetch_messages])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn crypto_module_compiles_and_roundtrips() {
        let alice_sk = "0xac0974bec39a17e36ba4a6b4d238ff944bacb478cbed5efcae784d7bf4f2ff80";
        let bob_sk = "0x59c6995e998f97a5a0044966f0945389dc9e86dae88c7a8412f4603b6b78690d";
        let alice_pub = crypto::public_key_from_private(alice_sk).unwrap();
        let bob_pub = crypto::public_key_from_private(bob_sk).unwrap();
        let payload = crypto::encrypt_message(alice_sk, &bob_pub, "ipc test").unwrap();
        let plain = crypto::decrypt_message(bob_sk, &alice_pub, &payload).unwrap();
        assert_eq!(plain, "ipc test");
    }
}
