# crypto-chat — Development Plan

> **Version v0.3** · 2026-11-25 · Owner: mirror-pan · Repo: `mirror-pan/crypto-chat`
>
> 这份文档是项目的 **construction blueprint（施工蓝图）**，不是宣传文案。
> 规则：每一个 milestone 完成后，必须回来更新状态栏和验收结果。没有写进这里的决策，不算数。

---

## 0a. v0.3 修订说明（相对 v0.2）

| 项目 | v0.2 | v0.3（当前有效） |
|---|---|---|
| 网络 | 本地 Anvil 已跑通 | **加 Sepolia 支持**，作为公网验证；本地链保留作日常开发 |
| 私钥来源 | CLI 参数 `--private-key`（进 shell history） | **支持 `CHAT_PRIVATE_KEY` 环境变量**，CLI 参数仅保留给 Anvil 公开测试 key |
| 部署文档 | 只有 `TESTING.md`（本地） | 新增 `chat-cli/SEPOLIA.md`，覆盖建钱包 / faucet / 部署 / 收发 / 浏览器验证 |
| 风险 D6 | "先本地后 Sepolia" | 保持；**切 Sepolia 不是替代本地链，是追加一次公网验证** |

---

## 0. v0.2 修订说明（相对 v0.1）

| 项目 | v0.1（已作废） | v0.2（当前有效） |
|---|---|---|
| 加密方案 | "先用公钥哈希做简化版，之后再升级成 ECDH" | **直接实现真正的 secp256k1 ECDH + AES-256-GCM，不写简化版**。避免同一段代码写两遍 |
| TypeScript 边界 | 模糊，导致 CLI 用 TS 和 Rust 各写了一遍 | **Phase 2 纯 Rust，零 TypeScript**。TS 只在 Phase 3 做 UI 时登场 |
| 身份概念 | "公钥即地址" | **纠正：Ethereum address ≠ public key**。address 是 pubkey 经 keccak256 后取末 20 字节，不可逆；ECDH 必须交换完整 uncompressed public key |
| 环境假设 | 假设新电脑是干净的，从零写安装步骤 | 记录本机的真实配置：WSL2 + 代理 7897 + rustup stable + Remote-WSL |
| Phase 2 状态 | "未开始" | 代码已落盘，编译错误已修复，**等待 `cargo test` 复验** |

---

## 1. Project definition（项目定义）

**一句话**：一个去中心化的端到端加密聊天协议——消息在用户自己的电脑上加密，通过 Ethereum 智能合约的 event log 传递，任何中间人都只能看到密文。

**工程目标**：借这个项目训练全英文软件工程流程（naming / comments / commits / docs），并理解 cryptography、EVM、异步 Rust、桌面 GUI 的完整链路。

**非目标（明确不做）**：
- 不做匿名网络层（Tor / mixnet）——本项目不承诺隐藏 IP。
- 不做前向安全（Double Ratchet）——现阶段是 static ECDH，见 §8 风险表。
- 不做移动端。Phase 3 只做桌面端。

---

## 2. Problem & scope（要解决的问题与边界）

| 传统软件的问题 | 本项目的应对 | 不能解决的问题 |
|---|---|---|
| 中心化服务器能看到聊天内容 | 本地加密，链上只有 ciphertext | — |
| 服务器记录"谁在跟谁聊"（metadata） | 用 wallet address 作身份，没有中心服务器存社交图谱 | RPC provider 仍能看到你的 IP |
| 服务可被审查 / 关停 | event log 由全球节点共同维护 | 区块链本身是公开账本，密文永久可见 |
| 消息被篡改 | AES-GCM 的 auth tag 提供 tamper detection | 无法阻止别人往链上发垃圾消息（spam） |

**原则**：不夸大隐私承诺。文档中必须同时写清楚"加密保护了什么"和"没有保护什么"。

---

## 3. System architecture（三层架构）

用奶茶店类比理解分工，但工程上按表格里的技术边界执行：

| Layer | 类比 | 技术 | 能看到明文的人 | 状态 |
|---|---|---|---|---|
| **Frontend** | 点单台 | React + TypeScript + Tauri | 本机用户 | Phase 3 |
| **Backend** | 后厨 | Rust（crypto + ethers-rs） | 只有本机 | Phase 2（当前） |
| **Chain** | 全城公开的告示板 | Solidity `Chat.sol` on Foundry / Sepolia | 所有人，但只见密文 | Phase 1 ✅ |

**Data flow（完整闭环）**：

```
Alice Frontend: type plaintext
        ↓
Alice Backend (Rust):
    ECDH(Alice SK, Bob PK) → SHA-256 → AES-256 key
    AES-256-GCM(key, random IV, plaintext) → payload
        ↓
Chat.sol: sendMessage(Bob address, payload)
        ↓
Event log: MessageSent(sender, recipient, bytes payload, timestamp)
        ↓
Bob Backend: filter events where recipient == Bob
        ↓
Bob Backend:
    ECDH(Bob SK, Alice PK) → same AES key
    AES-256-GCM decrypt(payload) → plaintext
        ↓
Bob Frontend: render chat bubble
```

**为什么私钥绝不能进 Frontend**：UI 层会被浏览器/ WebView 环境观察，一旦私钥暴露，E2EE 全部失效。所以 Rust backend 是唯一接触私钥的地方。这是 security boundary，不是代码风格偏好。

---

## 4. Decision log（已定决策，不反复横跳）

| # | Decision | Rationale | Rejected alternative |
|---|---|---|---|
| D1 | 合约用 **event log** 传消息，不用 `SSTORE` | `emit` 的 gas 成本远低于状态存储；消息本来就是 append-only | 把消息存进 contract storage |
| D2 | Phase 2 **只用 Rust** | 密码学和链上交互会被 Phase 3 的 Tauri backend 原样复用 | TypeScript + viem（已删除） |
| D3 | 直接用**真 ECDH**，不做简化版 | 简化版等于把代码写两遍，且会让人误以为已经安全 | "SHA-256(public key) 当 AES key"（公开值谁都能算） |
| D4 | 身份交换用 **uncompressed public key**（130 hex, `04` 开头） | address 是单向哈希，算不出共享密钥 | "线下交换地址即可" |
| D5 | 载荷格式 `[12-byte IV][ciphertext + 16-byte tag]` | AEAD 标准做法；IV 每条消息随机，tag 检测篡改 | 明文拼接 / 固定 IV |
| D6 | 本地 Anvil 先跑通，再切 Sepolia | 本地零延迟、可反复重置；Sepolia 免费且日志永久记录 | 直接上主网 |
| D7 | 合约开发用 **Foundry** | 测试用 Solidity 写，速度快，贴合底层 EVM | Hardhat |
| D8 | 公网上私钥只从 **`CHAT_PRIVATE_KEY` 环境变量**读，`--private-key` 仅用于 Anvil 公开 key | CLI 参数会进 shell history 和 `ps`；测试网 key 必须一次性、专用 | 继续用 CLI 参数传真实 key |
| D9 | Sepolia 用**公共 RPC 起步**（publicnode），不够稳再换个人 Alchemy/Infura | 零注册成本，几条测试消息足够 | 一开始就付费节点 |

---

## 5. Phase roadmap（阶段路线图）

### Phase 1 — Smart contract core ✅ DONE

- **Scope**：`Chat.sol` + `Chat.t.sol` + 本地部署
- **Deliverables**：`chat-core/src/Chat.sol`、`chat-core/test/Chat.t.sol`
- **Definition of Done**：
  - [x] `forge test -vv` — 3 tests passed
  - [x] `forge create ... --broadcast` — 部署成功，拿到 `Deployed to`
- **Note**：新电脑上 Anvil 重启后旧部署失效，Phase 2 测试前需要重新 deploy（见 `chat-cli/TESTING.md` §2）。已于 2026-11-25 17:05 在新电脑重部署成功，地址 `0x5FbDB2315678afecb367f032d93F642f64180aa3`，tx `0x28de2dd485b820f0cba0638e3ddfeaf6317bcf8984b38cf99a813ab3111be07e`。

### Phase 2 — Rust CLI & encryption layer ✅ DONE

- **Scope**：本地 E2EE + 通过 RPC 把密文中继到 Phase 1 合约。**没有 UI，没有 TS。**
- **Deliverables**：
  - `chat-cli/Cargo.toml` — dependency manifest
  - `chat-cli/src/crypto.rs` — ECDH / AES-256-GCM / `pubkey` 派生 + 2 unit tests
  - `chat-cli/src/main.rs` — 3 subcommands: `pubkey` / `send` / `decrypt`
  - `chat-cli/README.md`、`chat-cli/TESTING.md` — 全英文 docs
- **Current state**：
  - [x] 文件已写入 WSL 仓库
  - [x] Rust stable 已通过 rustup 安装；`Cargo.lock` 已生成（419 packages resolved）
  - [x] 编译错误已修复：补上 `tokio` 依赖 + 3 处 `.map_err(|e| eyre::eyre!(e))`
  - [x] 修复 E0716 临时值借用错误（`ContractCall` 先绑定局部变量再 `.send()`）
  - [x] **`cargo test` / `cargo build` 复验通过**（2026-11-25 16:54：`2 passed`；21:34 加裸公钥测试后 `3 passed`）
  - [x] 真实闭环：derive pubkey → send → `cast logs` 取 payload → decrypt 成功（2026-11-25 17:14：`[+] Plaintext: Hello Bob, this is E2EE over Ethereum`）
  - [x] 真实闭环：derive pubkey → send → 从 event log 取 payload → decrypt
  - [x] 切到 Sepolia 测试网 — **闭环通过**（2026-11-25 21:38）：合约地址 `0x21433635Df276caA6df2cD9F2489321797506cb0`（部署 tx `0x00d09b5bbd9ebda12466f5b5606f27bd6054c4f30420634d8c328bb15de69c80`）；消息 tx `0x733678687b8059026c05a3ba46116d43a84b5b252aa9490d84010d8b76c6758e`；Bob 解密得到 `Hello Bob, this is Sepolia E2EE`。Alice 私钥不进 CLI 参数，Bob 用独立终端持有自己的 key |
  - [x] 兼容 `cast wallet new --json` 的 64 字节裸 X||Y 公钥格式（新增 `raw_xy_public_key_from_cast_is_accepted` 单测，`cargo test` → 3 passed）
- **Definition of Done**：
  - [ ] `cargo test` → `2 passed`（`ecdh_roundtrip_succeeds`、`wrong_recipient_key_fails_to_decrypt`）
  - [ ] `cargo run -- send ...` 返回 tx hash
  - [ ] `cast receipt` 拿到 `MessageSent` 的 `encryptedContent`
  - [ ] `cargo run -- decrypt ...` 恢复出原文，且错钥匙解密失败

### Phase 3 — Desktop UI ⬜ NOT STARTED

- **Scope**：仿微信聊天窗口 + 实时事件监听
- **Tech**：Tauri + React + TypeScript + Tailwind CSS
- **Deliverables**：`chat-desktop/`，`src-tauri/` 复用 Phase 2 的 Rust 代码
- **Key work**：
  - WebSocket `eth_subscribe` 后台 listener（Tokio task）
  - Tauri IPC：Rust → React 推送 `new-chat-message` event
  - 左侧会话列表 + 右侧消息气泡 + 输入框
- **Definition of Done**：打开桌面窗口能发消息、对方消息自动弹出，私钥不出 Rust 层

### Phase 4 — Hardening & open-source packaging ⬜ NOT STARTED

- **Scope**：安全加固 + 全英文文档 + 规范提交历史
- **Must-do**：
  - 私钥从 CLI 参数改为 **environment variable / keystore file**（CLI 参数会进 shell history）
  - 自定义 error type 替代 `String`（替换当前的 `eyre!` 权宜之计）
  - 全英文 root `README.md` + architecture diagram + `LICENSE`
  - Conventional Commits 规范；`git log --oneline` 可读
- **Definition of Done**：别人照 README 能在干净机器上跑通全流程

---

## 6. Environment setup（本机真实配置）

> 这一节记录踩过的坑，换机器时直接照抄。

**Host**：Windows + WSL2 `Ubuntu-22.04`；VS Code Windows 版 + Remote-WSL + rust-analyzer。

**网络**：Clash proxy 端口 **7897**。WSL2 用 mirrored networking，`127.0.0.1:7897` 指向 Windows 宿主机。

```bash
# 1. Install Rust (rustup, NOT apt — apt 的 1.75 太老)
export https_proxy=http://127.0.0.1:7897 http_proxy=http://127.0.0.1:7897
curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh -s -- -y --profile minimal --default-toolchain stable
source $HOME/.cargo/env

# 2. Cargo 代理配置已写入 ~/.cargo/config.toml（下载 crates 用）
# 3. VS Code 必须在 WSL 终端里用 `code .` 打开（点号不能省）
```

**Foundry**（新电脑尚未确认）：
```bash
curl -L https://foundry.paradigm.xyz | bash
source ~/.bashrc && foundryup
forge --version
```

**已知坑**：
- `git clone` 卡住 → 检查代理；不要用 `127.0.0.1:7890`，本机是 7897。
- VS Code 打开错项目 → 敲的是 `code` 而不是 `code .`。
- `cargo` 用 apt 装会因版本过旧导致 ethers 编译失败 → 只用 rustup。

---

## 7. Engineering conventions（开发规范）

- **Naming / comments / docs / commits 一律英文**。例：`recipient`，不写 `shoujianren`。
- **Commit format**：`<type>(<scope>): <subject>`
  - `feat(crypto): implement ECDH shared secret derivation`
  - `fix(cli): add missing tokio dependency`
  - `docs: rewrite project plan for Phase 2`
- **多机器同步**：离开一台机器前 `git push`；到另一台先 `git pull`。
- **Secrets**：Anvil 测试私钥可以出现在文档里；**真实私钥绝不提交**。

---

## 8. Risk register（风险与已知限制）

| Risk | Severity | Current mitigation | Planned fix |
|---|---|---|---|
| Static ECDH → 没有前向安全（key 泄露则历史消息全泄） | High | 明确写在文档里，不宣称具备 forward secrecy | Phase 4 后评估 Double Ratchet / ephemeral keys |
| 私钥经 CLI 参数传入 → 进 shell history | High | 已修：`CHAT_PRIVATE_KEY` 环境变量读取，CLI 参数仅保留给 Anvil 公开 key（D8） | Phase 4 再评估 keystore |
| RPC provider 看到 IP（metadata leakage） | Medium | 本地 Anvil 阶段无外部 RPC | Phase 4 前评估 VPN / private RPC |
| 链上密文永久公开；无助记词/密钥则不可恢复 | Medium | 文档警示 | — |
| Sepolia 测试网可能被重置 / 限速 | Low | 先用本地 Anvil | 使用个人 Alchemy / Infura key |
| `String` error + `eyre!` 是权宜之计 | Low | 能编译、能跑 | Phase 4 定义 typed error enum |

---

## 9. Immediate next actions（立即下一步）

1. 在 WSL 的 `chat-cli/` 目录执行 `cargo test && cargo build`，把输出发给我。
2. 若通过 → 按 `chat-cli/TESTING.md` §2 重启 Anvil 并重新部署合约，记录新的 `Deployed to`。
3. 依次执行 §3–§6：派生双方公钥 → Alice `send` → `cast receipt` 取 payload → Bob `decrypt`。
4. 闭环通过后，把 Phase 2 的代码 commit 到 Git（英文 Conventional Commit）。
5. 再开一轮：把 `--rpc` 指向 Sepolia，验证测试网上 event log 同样可查。
6. 全部验收完，才允许开始 Phase 3。

> **纪律**：前一步的 DoD 没打勾，不进下一步。这是这份计划存在的意义。
