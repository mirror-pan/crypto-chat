mod chain;
mod crypto;

use std::env;
use std::sync::Mutex;

use tauri::State;

// ---------------------------------------------------------------------------
// Security model (login gate):
// - The private key is entered ONCE in the UI, sent to Rust via a single IPC
//   call, and stored ONLY in this process's memory (AppState).
// - It is never written to disk, never logged, never returned to the UI.
// - `lock()` zeroes it. Closing the app discards it.
// - Commands below read from memory first. The CHAT_PRIVATE_KEY environment
//   variable is kept only as a fallback for terminal-driven dev sessions.
// ---------------------------------------------------------------------------

#[derive(Default)]
struct AppState {
    private_key: Mutex<Option<String>>,
}

impl AppState {
    fn read_key(&self) -> Result<String, String> {
        if let Some(key) = self.private_key.lock().unwrap().clone() {
            return Ok(key);
        }
        env::var("CHAT_PRIVATE_KEY")
            .map_err(|_| "Locked. Enter your private key to continue.".to_string())
    }

    fn set_key(&self, key: String) -> Result<(), String> {
        let mut guard = self.private_key.lock().unwrap();
        *guard = Some(key);
        Ok(())
    }

    fn clear_key(&self) {
        // Dropping the String releases the buffer; the OS may reuse the pages.
        *self.private_key.lock().unwrap() = None;
    }

    fn is_unlocked(&self) -> bool {
        self.private_key.lock().unwrap().is_some() || env::var("CHAT_PRIVATE_KEY").is_ok()
    }
}

/// Validate a private key and keep it in memory. Returns only public identity.
#[tauri::command]
fn unlock(private_key: String, state: State<'_, AppState>) -> Result<chain::Identity, String> {
    let key = private_key.trim().to_string();
    // Validate first — an invalid key must never be stored.
    let identity = chain::identity_from_private(&key)?;
    state.set_key(key)?;
    Ok(identity)
}

/// Erase the private key from memory. The UI must return to the login gate.
#[tauri::command]
fn lock(state: State<'_, AppState>) -> Result<(), String> {
    state.clear_key();
    Ok(())
}

/// Report whether a key is currently available (in memory or env fallback).
#[tauri::command]
fn is_unlocked(state: State<'_, AppState>) -> Result<bool, String> {
    Ok(state.is_unlocked())
}

/// Return the public identity of the signed-in key. Never returns the key.
#[tauri::command]
fn identity(state: State<'_, AppState>) -> Result<chain::Identity, String> {
    let key = state.read_key()?;
    chain::identity_from_private(&key)
}

/// Return our uncompressed public key so the UI can display it.
#[tauri::command]
fn get_pubkey(state: State<'_, AppState>) -> Result<String, String> {
    let private_key = state.read_key()?;
    crypto::public_key_from_private(&private_key)
}

/// Encrypt a message locally and relay the ciphertext to Chat.sol on Sepolia.
/// The UI passes only recipient address, peer public key, and plaintext.
#[tauri::command]
async fn send_message(
    recipient: String,
    peer_pubkey: String,
    message: String,
    state: State<'_, AppState>,
) -> Result<String, String> {
    let private_key = state.read_key()?;
    chain::send_encrypted(&private_key, &recipient, &peer_pubkey, &message).await
}

/// Fetch and decrypt conversation history with a peer from on-chain logs.
#[tauri::command]
async fn fetch_messages(
    peer_address: String,
    peer_pubkey: String,
    state: State<'_, AppState>,
) -> Result<Vec<chain::FetchedMessage>, String> {
    let private_key = state.read_key()?;
    chain::fetch_messages(&private_key, &peer_address, &peer_pubkey).await
}

/// Publish our public key to the on-chain registry. Called once after unlock.
/// Kept separate from `unlock` so an RPC failure never blocks login.
#[tauri::command]
async fn register_pubkey(state: State<'_, AppState>) -> Result<chain::Registration, String> {
    let private_key = state.read_key()?;
    chain::register_pubkey(&private_key).await
}

/// Look up any address's registered public key from the on-chain registry.
#[tauri::command]
async fn lookup_pubkey(address: String) -> Result<String, String> {
    chain::lookup_pubkey(&address).await
}

/// Discover contacts we have exchanged messages with, resolving their keys.
#[tauri::command]
async fn discover_contacts(state: State<'_, AppState>) -> Result<Vec<chain::ContactInfo>, String> {
    let private_key = state.read_key()?;
    chain::discover_contacts(&private_key).await
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_opener::init())
        .manage(AppState::default())
        .invoke_handler(tauri::generate_handler![
            unlock,
            lock,
            is_unlocked,
            identity,
            get_pubkey,
            send_message,
            fetch_messages,
            register_pubkey,
            lookup_pubkey,
            discover_contacts
        ])
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

    #[test]
    fn app_state_holds_and_clears_key() {
        let state = AppState::default();
        assert!(state.private_key.lock().unwrap().is_none());
        state.set_key("0xabc".to_string()).unwrap();
        assert_eq!(state.read_key().unwrap(), "0xabc");
        state.clear_key();
        assert!(state.private_key.lock().unwrap().is_none());
    }
}
