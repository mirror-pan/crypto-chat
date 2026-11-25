# Phase 2 verification and end-to-end test

This document walks through verifying the Rust CLI from compilation to a full
encrypt → relay → decrypt cycle on a local chain.

## 1. Compile and run unit tests

```bash
cd ~/crypto-chat/chat-cli
cargo test
cargo build
```

`cargo test` must report 2 passed tests from `crypto.rs`:
- `ecdh_roundtrip_succeeds` — Alice encrypts, Bob decrypts, plaintext recovered.
- `wrong_recipient_key_fails_to_decrypt` — Charlie cannot decrypt Bob's message.

If either fails, the ECDH or AEAD layer is broken. Do not proceed.

## 2. Prepare the local chain

Terminal A — start Anvil and leave it running:

```bash
anvil
```

Terminal B — redeploy the Phase 1 contract (the old deployment was lost when
the machine changed):

```bash
cd ~/crypto-chat/chat-core
forge create src/Chat.sol:Chat \
  --rpc-url http://127.0.0.1:8545 \
  --private-key 0xac0974bec39a17e36ba4a6b4d238ff944bacb478cbed5efcae784d7bf4f2ff80 \
  --broadcast
```

Record the `Deployed to:` address. It replaces `<CONTRACT>` below.

## 3. Derive public keys (out-of-band exchange simulation)

Alice's key pair (Anvil account 0):

```bash
cargo run -- pubkey --private-key 0xac0974bec39a17e36ba4a6b4d238ff944bacb478cbed5efcae784d7bf4f2ff80
```

Bob's key pair (Anvil account 1):

```bash
cargo run -- pubkey --private-key 0x59c6995e998f97a5a0044966f0945389dc9e86dae88c7a8412f4603b6b78690d
```

Each output is a 130-character hex string starting with `04`. In a real
deployment these are exchanged in person — the Ethereum address is NOT enough.

## 4. Send an encrypted message

Alice sends to Bob. Use Bob's uncompressed public key from step 3 as
`<BOB_PUBKEY>`, and Bob's address
`0x70997970C51812dc3A010C7d01b50e0d17dc79C8` as `<BOB_ADDRESS>`.

```bash
cargo run -- send \
  --recipient <BOB_ADDRESS> \
  --peer-pubkey <BOB_PUBKEY> \
  --message "Hello Bob, this is E2EE over Ethereum" \
  --contract <CONTRACT> \
  --private-key 0xac0974bec39a17e36ba4a6b4d238ff944bacb478cbed5efcae784d7bf4f2ff80
```

The CLI prints the transaction hash. The plaintext never appears in the
transaction — only the ciphertext payload.

## 5. Capture the on-chain payload

The `MessageSent` event contains the `bytes encryptedContent`. Extract it from
the transaction receipt:

```bash
cast receipt <TX_HASH> --rpc-url http://127.0.0.1:8545
```

Copy the `data` field of the `MessageSent` log. It is the hex payload beginning
with the 12-byte IV.

## 6. Decrypt as Bob

Use Alice's public key from step 3 as `<ALICE_PUBKEY>`:

```bash
cargo run -- decrypt \
  --sender-pubkey <ALICE_PUBKEY> \
  --payload <PAYLOAD_FROM_EVENT> \
  --private-key 0x59c6995e998f97a5a0044966f0945389dc9e86dae88c7a8412f4603b6b78690d
```

Expected output:

```
[+] Plaintext: Hello Bob, this is E2EE over Ethereum
```

If this matches, Phase 2 is functionally complete: the ciphertext travelled
through the smart contract, and only the intended recipient could recover it.

## Security notes

- The keys above are public Anvil test keys. Never reuse them on a real network.
- Private keys passed via CLI arguments appear in shell history. Before moving
  to Sepolia, switch to environment variables (planned for Phase 4).
- ECDH here is static (long-term keys only). Forward secrecy requires ephemeral
  keys — a Double Ratchet style upgrade, tracked as a later enhancement.
