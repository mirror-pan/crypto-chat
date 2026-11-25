# Running the protocol on Sepolia

This guide moves the Phase 1 contract and the Phase 2 CLI from the local Anvil
chain to the Sepolia public testnet. The encryption code does not change: only
the RPC endpoint, the contract address, and the key source change.

Sepolia is a **public** chain. Do not use a private key that holds real funds,
and do not send any message you are not comfortable having permanently visible
as ciphertext. ECDH + AES-GCM protects the plaintext; it does not hide the
sender address, recipient address, timestamp, or message size.

## 0. Prerequisites

From the `chat-cli/` directory, confirm the code still compiles:

```bash
cd ~/crypto-chat/chat-cli
cargo test && cargo build
```

Expected: `2 passed; 0 failed` and a successful build.

## 1. Create a dedicated test wallet

Generate a brand-new key pair. This wallet is for testing only. Do not reuse
an Anvil key: those private keys are publicly known, so anyone could impersonate
that address on Sepolia.

```bash
cd ~ && cast wallet new --json > sepolia-wallet.json && chmod 600 sepolia-wallet.json && cat sepolia-wallet.json
```

This creates one JSON line holding the address and private key in your home
directory (outside the git repo), locks the file to owner-only permissions, and
prints it once. Never put a mainnet key here, and never share the private key
with anyone — only the address is needed for the faucet.

## 2. Get an RPC endpoint

Two options:

- **No-signup public RPC (simplest)**: `https://ethereum-sepolia-rpc.publicnode.com`
  Works for reading and sending, but is rate-limited and shared. Fine for a few
  test messages.
- **Personal endpoint (more reliable)**: create a free account on Alchemy or
  Infura, make a Sepolia app, and copy its HTTPS URL.

Set it as an environment variable in the terminal you will use:

```bash
export SEPOLIA_RPC="https://ethereum-sepolia-rpc.publicnode.com"
```

## 3. Fund the wallet with test ETH

The faucet asks for the **address** from step 1, never the private key.

- Easiest with no mainnet ETH and no account: the PoW faucet
  `https://sepolia-faucet.pk910.de/` — paste the address, start mining, wait
  until the balance reaches the minimum, then claim.
- Alternatives: Google Cloud Web3 faucet or LearnWeb3 (both require a login).

Verify the balance before deploying:

```bash
cast balance <YOUR_ADDRESS> --rpc-url $SEPOLIA_RPC
```

A non-zero result means the funding worked. Sepolia ETH has no monetary value;
never buy it from anyone.

## 4. Export the key the safe way

The CLI now reads `CHAT_PRIVATE_KEY` from the environment, so the key never
appears in shell history or in the process argument list. Set it like this:

```bash
read -rsp "Private key: " CHAT_PRIVATE_KEY && echo && export CHAT_PRIVATE_KEY
```

Paste the key at the prompt and press Enter. The input is hidden. Do not use
`export CHAT_PRIVATE_KEY=0x...` directly, because that command itself is stored
in shell history.

Derive the address to confirm the key loaded correctly:

```bash
cargo run -- pubkey
```

A 130-character hex string starting with `04` is the uncompressed public key.
Keep it for the decrypt step. To check the matching address:

```bash
cast wallet address --private-key "$CHAT_PRIVATE_KEY"
```

## 5. Deploy Chat.sol to Sepolia

For this one Foundry command the key must be passed as an argument. Because you
type `"$CHAT_PRIVATE_KEY"` rather than the literal key, shell history stores only
the variable name — the key itself is never written there. It is briefly visible
in the process list while `forge` runs, which is acceptable for a throwaway
test wallet holding no real funds.

```bash
cd ~/crypto-chat/chat-core
forge create src/Chat.sol:Chat \
  --rpc-url "$SEPOLIA_RPC" \
  --private-key "$CHAT_PRIVATE_KEY" \
  --broadcast
```

Copy the `Deployed to:` address. This is `<CONTRACT>` below. Record the tx hash
too. Contract deployment is a one-time operation: every later message reuses
this same address.

## 6. Send an encrypted message

You need the peer's Ethereum **address** (for routing) and their uncompressed
**public key** (for ECDH). An address alone is not enough.

```bash
cd ~/crypto-chat/chat-cli
cargo run -- send \
  --recipient <BOB_ADDRESS> \
  --peer-pubkey <BOB_PUBKEY> \
  --message "Hello Bob, this is Sepolia E2EE" \
  --rpc "$SEPOLIA_RPC" \
  --contract <CONTRACT>
```

The CLI prints a transaction hash. The chain stores only the ciphertext.
If your peer is on the same machine with another test wallet, repeat step 1 and
step 3 for that wallet, and export its key in a second terminal before running
`decrypt`.

## 7. Capture the payload and decrypt

Sepolia is public, so the event can be read by anyone:

```bash
cast receipt <TX_HASH> --rpc-url "$SEPOLIA_RPC"
```

Find the `MessageSent` log and its `data` field. As on Anvil, `data` is ABI
encoded: `[offset][timestamp][length][payload][padding]`. Extract the payload
bytes between the length word and the trailing padding — do not feed the raw
`data` field into the decryptor.

Then decrypt locally with the recipient key:

```bash
cargo run -- decrypt \
  --sender-pubkey <ALICE_PUBKEY> \
  --payload <PAYLOAD_FROM_EVENT>
```

Both commands pick up `CHAT_PRIVATE_KEY` from the environment. Expected result:

```
[+] Plaintext: Hello Bob, this is Sepolia E2EE
```

## 8. Verify on the block explorer

Open `https://sepolia.etherscan.io/address/<CONTRACT>` and confirm the
`sendMessage` transaction is present. This is proof that the log was written to
a public chain, not just to your local Anvil instance.

## Troubleshooting

| Symptom | Cause | Fix |
|---|---|---|
| `insufficient funds for gas` | Wallet not funded, or wrong network | Recheck `cast balance` with `$SEPOLIA_RPC` |
| `nonce too low` / `replacement transaction underpriced` | Public RPC lag | Wait a block and retry |
| `no private key supplied` | Env var not exported in this terminal | Re-run step 4 in the same terminal |
| `could not decode` during decrypt | Raw `data` used instead of the payload slice | Re-extract per step 7 |
| 403 / rate limited | Public RPC overloaded | Use a personal Alchemy/Infura endpoint |
