# chat-cli

Command-line client for the crypto-chat protocol (Phase 2).

It encrypts messages locally with **ECDH over secp256k1** and **AES-256-GCM**,
then relays only the ciphertext to the `Chat.sol` contract via an Ethereum RPC.
The plaintext never leaves your machine.

## Prerequisites

- Anvil running at `http://127.0.0.1:8545`
- `Chat.sol` deployed (see `../chat-core`)

## Build and test

```bash
cargo test          # run the crypto unit tests
cargo build         # compile the CLI
```

## Usage

### 1. Exchange public keys out-of-band

Ethereum addresses are NOT public keys. Derive your uncompressed public key and
give it to your peer in person:

```bash
cargo run -- pubkey --private-key 0xYOUR_PRIVATE_KEY
```

### 2. Send an encrypted message

```bash
cargo run -- send \
  --recipient 0x70997970C51812dc3A010C7d01b50e0d17dc79C8 \
  --peer-pubkey 04YOUR_PEER_PUBLIC_KEY \
  --message "Hello from the Rust client" \
  --contract 0x5FbDB2315678afecb367f032d93F642f64180aa3 \
  --private-key 0xYOUR_PRIVATE_KEY
```

### 3. Decrypt a payload captured from the event log

```bash
cargo run -- decrypt \
  --sender-pubkey 04SENDER_PUBLIC_KEY \
  --payload 0xCAPTURED_BYTES \
  --private-key 0xYOUR_PRIVATE_KEY
```

## Wire format

The `bytes` payload stored in the `MessageSent` event is:

```
[ 0 .. 12 ]  random IV / nonce
[ 12 ..   ]  AES-256-GCM ciphertext + 16-byte auth tag
```
