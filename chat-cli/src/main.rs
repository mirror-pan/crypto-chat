//! chat-cli — command line client for the crypto-chat protocol.
//!
//! Phase 2 scope: local E2EE encryption plus on-chain message relay.
//! No TypeScript, no UI. Just Rust talking to the smart contract deployed in Phase 1.

use clap::{Parser, Subcommand};
use ethers::prelude::*;
use ethers::signers::Signer;
use std::sync::Arc;

mod crypto;

// ABI generated from the Chat.sol contract written in Phase 1.
// The JSON form is used here so the signature matches the compiler output exactly.
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

#[derive(Parser)]
#[command(name = "chat-cli")]
#[command(about = "Decentralized E2EE chat CLI over Ethereum", long_about = None)]
struct Cli {
    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand)]
enum Commands {
    /// Derive your uncompressed public key to share with a peer
    Pubkey {
        /// Your private key (hex, 0x-prefixed)
        #[arg(long)]
        private_key: String,
    },

    /// Encrypt a message locally and relay the ciphertext on-chain
    Send {
        /// Recipient Ethereum address (0x...)
        #[arg(long)]
        recipient: String,
        /// Recipient's uncompressed public key (hex, starts with 04)
        #[arg(long)]
        peer_pubkey: String,
        /// Plaintext message to encrypt
        #[arg(long)]
        message: String,
        /// Ethereum RPC endpoint
        #[arg(long, default_value = "http://127.0.0.1:8545")]
        rpc: String,
        /// Deployed Chat.sol contract address
        #[arg(long)]
        contract: String,
        /// Your private key (hex, 0x-prefixed)
        #[arg(long)]
        private_key: String,
    },

    /// Decrypt a captured on-chain payload locally (no RPC needed)
    Decrypt {
        /// Sender's uncompressed public key (hex, starts with 04)
        #[arg(long)]
        sender_pubkey: String,
        /// The on-chain bytes payload (hex, from the event log)
        #[arg(long)]
        payload: String,
        /// Your private key (hex, 0x-prefixed)
        #[arg(long)]
        private_key: String,
    },
}

#[tokio::main]
async fn main() -> eyre::Result<()> {
    let cli = Cli::parse();

    match cli.command {
        Commands::Pubkey { private_key } => {
            let public = crypto::public_key_from_private(&private_key)
                .map_err(|e| eyre::eyre!(e))?;
            println!("[*] Your uncompressed public key (share this with your peer):");
            println!("{}", public);
        }

        Commands::Send {
            recipient,
            peer_pubkey,
            message,
            rpc,
            contract,
            private_key,
        } => {
            println!("[*] Connecting to Ethereum node at {}...", rpc);
            let provider = Provider::<Http>::try_from(rpc.as_str())?;
            let chain_id = provider.get_chainid().await?.as_u64();

            let wallet: LocalWallet = private_key.parse::<LocalWallet>()?.with_chain_id(chain_id);
            let client = Arc::new(SignerMiddleware::new(provider, wallet));

            // Step 1: encrypt off-chain. The chain never sees the plaintext.
            println!("[*] Encrypting message locally with ECDH + AES-256-GCM...");
            let payload = crypto::encrypt_message(&private_key, &peer_pubkey, &message)
                .map_err(|e| eyre::eyre!(e))?;
            println!("[*] Ciphertext payload size: {} bytes", payload.len());

            let recipient_address: Address = recipient.parse()?;
            let contract_address: Address = contract.parse()?;

            // Step 2: relay the ciphertext to the contract's event log.
            let chat = ChatContract::new(contract_address, client);
            println!("[*] Dispatching transaction to Chat.sol...");

            // Bind the ContractCall to a local variable first: `send(&self)` returns
            // a PendingTx borrowing the call, so a temporary would be dropped while
            // `pending_tx` is still in use (E0716).
            let call = chat.send_message(recipient_address, payload.into());
            let pending_tx = call.send().await?;

            println!("[+] Transaction broadcast successfully.");
            println!("[+] Tx hash: {:?}", pending_tx.tx_hash());
            println!("[+] Check the MessageSent event in your block explorer.");
        }

        Commands::Decrypt {
            sender_pubkey,
            payload,
            private_key,
        } => {
            println!("[*] Decrypting payload locally...");
            let payload_bytes = hex::decode(payload.trim_start_matches("0x"))?;
            let plaintext = crypto::decrypt_message(&private_key, &sender_pubkey, &payload_bytes)
                .map_err(|e| eyre::eyre!(e))?;
            println!("[+] Plaintext: {}", plaintext);
        }
    }

    Ok(())
}
