# Public key registry and multi-account testing

This document covers the contract upgrade that adds an on-chain public key
registry, plus the procedure for testing with several accounts.

## Why the registry exists

`Chat.sol` stores only ciphertext in `MessageSent` events. To encrypt a message,
the sender needs the recipient's public key, but an Ethereum address is a hash
of the key and cannot be reversed. The registry closes that gap:

- `registerPublicKey(bytes)` publishes the caller's 64-byte `X || Y` key.
- The contract computes `address(uint160(uint256(keccak256(key))))` and requires
  it to equal `msg.sender`. Registering somebody else's key is impossible.
- `publicKeys(address)` is a public getter used to resolve peers on the fly.

The app calls `register_pubkey` once after login. Registration is idempotent:
it reads `publicKeys(our address)` first and skips the transaction when the
key already matches, so repeated logins spend no gas and cannot hit a nonce
conflict (`replacement transaction underpriced`). On startup it also calls
`discover_contacts`, which scans `MessageSent` logs for addresses we have
exchanged messages with and resolves their keys from the registry. Contacts
can also be added manually by address through the sidebar input.

Only public data (addresses, public keys, contact notes) is cached in the
browser's localStorage. The private key is never persisted anywhere.

## Step 1: verify the contract locally

```bash
cd ~/crypto-chat/chat-core && forge test
```

Expected: 9 tests pass (4 message-relay + 5 registry).

## Step 2: deploy the new contract to Sepolia

Use the same funded test wallet as before. This is a NEW deployment — the old
contract and its history remain on chain at the old address.

```bash
cd ~/crypto-chat/chat-core
export SEPOLIA_RPC="https://ethereum-sepolia-rpc.publicnode.com"
read -rsp "Private key: " CHAT_PRIVATE_KEY && echo && export CHAT_PRIVATE_KEY
forge create src/Chat.sol:Chat --rpc-url "$SEPOLIA_RPC" --private-key "$CHAT_PRIVATE_KEY" --broadcast
```

Record two values from the output:

- `Deployed to:` — the new contract address
- deployment block number (shown in the tx receipt, or query with
  `cast receipt <txhash> --rpc-url "$SEPOLIA_RPC" | grep blockNumber`)

## Step 3: hardcoded config (already applied)

`chat-desktop/src-tauri/src/chain.rs` now points at the deployed contract:

```rust
const DEFAULT_CONTRACT: &str = "0x3C639E282571A994536cC590D583C5EF9b0425B3";
const DEFAULT_FROM_BLOCK: u64 = 12216488;
```

Deployment tx: `0x3d82f84575b8e99fc13b330f778b95ef039e43dcd3a1f45844b0779cd42f736d`,
block `0xba690c` = 12216588. `DEFAULT_FROM_BLOCK` is 100 blocks earlier so
`eth_getLogs` stays within the public RPC's range limit.

These can be overridden at runtime with `CHAT_CONTRACT` / `CHAT_FROM_BLOCK` /
`CHAT_RPC`. Verify compilation:

```bash
cd ~/crypto-chat/chat-desktop/src-tauri && cargo test
```

Expected: 5 tests pass (3 crypto in `crypto.rs` + 2 state in `lib.rs`).

## Step 4: prepare test accounts

Generate as many wallets as needed:

```bash
cast wallet new --json > ~/test-account-2.json && chmod 600 ~/test-account-2.json
```

Fund each one from the already-funded wallet. `0.01` test ETH is plenty for
many messages:

```bash
cast send <NEW_WALLET_ADDRESS> --value 0.01ether \
  --rpc-url "$SEPOLIA_RPC" --private-key "$CHAT_PRIVATE_KEY"
```

Check with:

```bash
cast balance <NEW_WALLET_ADDRESS> --rpc-url "$SEPOLIA_RPC"
```

The faucet is not needed for every account. PoW faucet, if ever required:
`https://sepolia-faucet.pk910.de/`

## Step 5: end-to-end test on one machine

### Why port 1420 blocks a second dev instance

`tauri.conf.json` pins `devUrl` to `http://localhost:1420` and `vite.config.ts`
sets `strictPort: true`. The first `npm run tauri dev` owns that port; a second
one exits with "Port 1420 is already in use". Only the frontend dev server is
affected — the contract, accounts, and chain queries are independent of it.

Contract configuration is read from environment variables at runtime:
`CHAT_RPC`, `CHAT_CONTRACT`, `CHAT_FROM_BLOCK`. Set these in every terminal
that launches an app instance so all instances use the new contract.

### Option A — one dev server, rotate identities (recommended)

This exercises the full send/receive path because the app has no real-time
push yet: every message is read back from chain on demand.

1. Log in as account A. Confirm `Public key registered · tx …` in the sidebar.
2. Click Lock. Log in as account B. Confirm its registration.
3. As B, add A by address in the sidebar. Send a message to A.
4. Click Lock. Log in as A. `discover_contacts` should list B automatically.
5. Select B and confirm the message decrypts. Reply to B.
6. Lock, log in as B again, and confirm the reply appears.

Repeat with account C. Only one process ever holds a private key.

### Option B — three real windows from a production build

The built binary bundles the frontend, so no dev server and no port 1420 is
involved. Build once, then launch the binary from three terminals:

```bash
cd ~/crypto-chat/chat-desktop && npm run tauri build
# in each terminal, after setting CHAT_CONTRACT / CHAT_FROM_BLOCK / CHAT_RPC:
cd ~/crypto-chat/chat-desktop
./src-tauri/target/release/chat-desktop
```

Each process has its own in-memory key. On Windows use
`src-tauri/target/release/chat-desktop.exe`. Rebuild after any source change;
the dev server is not used in this mode.

## Known limitations

- The contract address is compiled in. Distributing the app means either
  shipping the official address or adding a configuration UI.
- Contact notes are local to each machine/browser profile. The keys and
  addresses are recoverable from chain, but display names are not.
- A keystore (encrypted key file plus unlock password) is still required
  before the app can be shipped as a double-clickable exe. The current login
  gate holds the key in process memory only.
