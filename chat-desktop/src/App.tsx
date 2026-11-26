import { useEffect, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import "./App.css";

// ---------------------------------------------------------------------------
// Phase 3: React UI + Rust crypto core via Tauri IPC.
// The UI holds ONLY public data (addresses, public keys, plaintext it displays).
// Private keys, encryption, and RPC never touch this layer — they live in Rust.
// ---------------------------------------------------------------------------

type Contact = {
  id: string;
  name: string;
  address: string;
  fullAddress: string;
  pubkey: string; // uncompressed (04...) or raw X||Y — public, safe to keep in UI
  preview: string;
  color: string;
  demo?: boolean; // demo contacts have no real key; sending to them is blocked
};

type Message = {
  id: number;
  contactId: string;
  from: "me" | "them";
  text: string;
  time: string;
  txHash?: string;
};

// Real peer from Phase 2 Sepolia round-trip. Address and pubkey were verified
// to match (address = keccak256(pubkey)[-20:]).
const INITIAL_CONTACTS: Contact[] = [
  {
    id: "bob",
    name: "Bob",
    address: "0x8649...2DBc7",
    fullAddress: "0x86497702dbaE7ea5d259bE4cD5d72A6d5082DBc7",
    pubkey: "0xdb4d95fbe324c59aaa5519e02fe24fa8b25970cd23bbef5d3b3036e5cfa79d7d9ad9a24694fe7885bc2036b47bcc5e95fe9d2fcbf8e0f79d3608d3a3f2f51f87",
    preview: "Real peer · E2EE on Sepolia",
    color: "bg-emerald-500",
  },
  {
    id: "alice",
    name: "Alice",
    address: "0xC22C...437e2",
    fullAddress: "0xC22C5F1A8dED04ABB16B3A85A8c1832c221437e2",
    pubkey: "",
    preview: "Demo contact (this is you)",
    color: "bg-sky-500",
    demo: true,
  },
  {
    id: "carol",
    name: "Carol",
    address: "0x1234...abcd",
    fullAddress: "",
    pubkey: "",
    preview: "Demo contact",
    color: "bg-violet-500",
    demo: true,
  },
];

// Shape returned by the Rust `fetch_messages` command (decrypted plaintext + tx).
type FetchedMessage = {
  from: "me" | "them";
  text: string;
  tx_hash: string;
  timestamp: number;
};

// No hardcoded message history: on startup we load real messages from the chain.
const INITIAL_MESSAGES: Message[] = [];

function App() {
  const [contacts] = useState<Contact[]>(INITIAL_CONTACTS);
  const [messages, setMessages] = useState<Message[]>(INITIAL_MESSAGES);
  const [selectedId, setSelectedId] = useState("bob");
  const [draft, setDraft] = useState("");
  const [myPubkey, setMyPubkey] = useState("");
  const [pubkeyError, setPubkeyError] = useState("");
  const [sending, setSending] = useState(false);
  const [sendError, setSendError] = useState("");
  const [loading, setLoading] = useState(false);

  const activeContact = contacts.find((c) => c.id === selectedId) ?? contacts[0];
  const visibleMessages = messages.filter((m) => m.contactId === selectedId);

  // Load real conversation from the chain. This is what makes refresh safe:
  // messages live on Sepolia, not in React state.
  async function loadMessages(contact: Contact) {
    if (contact.demo || !contact.pubkey || !contact.fullAddress) {
      setMessages([]);
      return;
    }
    setLoading(true);
    try {
      const fetched = await invoke<FetchedMessage[]>("fetch_messages", {
        peerAddress: contact.fullAddress,
        peerPubkey: contact.pubkey,
      });
      // Rust returns oldest-first; keep that order. Timestamp -> local time label.
      setMessages(
        fetched.map((m, i) => ({
          id: i + 1,
          contactId: contact.id,
          from: m.from,
          text: m.text,
          time: new Date(m.timestamp * 1000).toLocaleTimeString([], {
            hour: "2-digit",
            minute: "2-digit",
          }),
          txHash: m.tx_hash,
        })),
      );
    } catch (e) {
      setSendError("Could not load messages: " + String(e));
    } finally {
      setLoading(false);
    }
  }

  // Run once on mount and whenever the selected contact changes.
  useEffect(() => {
    loadMessages(activeContact);
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [selectedId]);

  async function showMyPubkey() {
    try {
      const key = await invoke<string>("get_pubkey");
      setMyPubkey(key);
      setPubkeyError("");
    } catch (e) {
      setPubkeyError(String(e));
      setMyPubkey("");
    }
  }

  // Send goes through Rust: plaintext -> ECDH+AES-GCM -> Chat.sol event log.
  async function sendMessage() {
    const text = draft.trim();
    if (!text || sending) return;

    if (activeContact.demo || !activeContact.pubkey || !activeContact.fullAddress) {
      setSendError("Demo contact — no real public key. Select Bob to send an encrypted message.");
      return;
    }

    setSending(true);
    setSendError("");
    try {
      await invoke<string>("send_message", {
        recipient: activeContact.fullAddress,
        peerPubkey: activeContact.pubkey,
        message: text,
      });
      setDraft("");
      // Reload from chain so the message (and its real tx hash) comes back
      // from the canonical source — same path a refresh would take.
      await loadMessages(activeContact);
    } catch (e) {
      setSendError(String(e));
    } finally {
      setSending(false);
    }
  }

  return (
    <div className="flex h-screen w-screen overflow-hidden bg-slate-100 text-slate-800">
      {/* Sidebar: contact list */}
      <aside className="flex w-72 shrink-0 flex-col border-r border-slate-200 bg-white">
        <div className="border-b border-slate-200 px-5 py-4">
          <h1 className="text-lg font-semibold tracking-tight">crypto-chat</h1>
          <p className="mt-0.5 text-xs text-slate-400">E2EE over Ethereum · Sepolia</p>
        </div>

        <div className="px-3 py-3">
          <div className="rounded-lg bg-slate-100 px-3 py-2 text-sm text-slate-400">Search contacts</div>
        </div>

        <div className="flex-1 overflow-y-auto">
          {contacts.map((contact) => (
            <button
              key={contact.id}
              onClick={() => { setSelectedId(contact.id); setSendError(""); }}
              className={`flex w-full items-center gap-3 px-4 py-3 text-left transition-colors ${
                contact.id === selectedId ? "bg-emerald-50" : "hover:bg-slate-50"
              }`}
            >
              <span
                className={`flex h-10 w-10 shrink-0 items-center justify-center rounded-full text-sm font-semibold text-white ${contact.color}`}
              >
                {contact.name[0]}
              </span>
              <span className="min-w-0 flex-1">
                <span className="flex items-baseline justify-between gap-2">
                  <span className="truncate font-medium">{contact.name}</span>
                  <span className="shrink-0 text-[10px] text-slate-400">
                    {contact.demo ? "demo" : "real"}
                  </span>
                </span>
                <span className="block truncate text-xs text-slate-400">{contact.address}</span>
                <span className="block truncate text-xs text-slate-500">{contact.preview}</span>
              </span>
            </button>
          ))}
        </div>

        <div className="border-t border-slate-200 px-4 py-3">
          <button
            onClick={showMyPubkey}
            className="w-full rounded-lg border border-emerald-200 bg-emerald-50 px-3 py-2 text-xs font-medium text-emerald-700 transition-colors hover:bg-emerald-100"
          >
            Show my public key (from Rust)
          </button>
          {myPubkey && (
            <div className="mt-2 break-all rounded-lg bg-slate-50 p-2 font-mono text-[10px] leading-relaxed text-slate-600">
              {myPubkey}
            </div>
          )}
          {pubkeyError && (
            <div className="mt-2 rounded-lg bg-red-50 p-2 text-[10px] leading-relaxed text-red-600">
              {pubkeyError}
            </div>
          )}
          <p className="mt-2 text-[10px] text-slate-400">Private key stays in Rust — never in the UI</p>
        </div>
      </aside>

      {/* Main chat panel */}
      <main className="flex min-w-0 flex-1 flex-col">
        <header className="flex items-center gap-3 border-b border-slate-200 bg-white px-6 py-4">
          <span
            className={`flex h-9 w-9 items-center justify-center rounded-full text-sm font-semibold text-white ${activeContact.color}`}
          >
            {activeContact.name[0]}
          </span>
          <div>
            <div className="font-semibold">{activeContact.name}</div>
            <div className="text-xs text-slate-400">{activeContact.address}</div>
          </div>
          <span className="ml-auto rounded-full bg-emerald-100 px-3 py-1 text-xs text-emerald-700">
            {activeContact.demo ? "Demo only" : "E2EE ready"}
          </span>
        </header>

        {/* Message thread */}
        <div className="flex-1 space-y-4 overflow-y-auto bg-slate-50 px-6 py-6">
          {loading && (
            <div className="text-center text-xs text-slate-400">Loading messages from chain…</div>
          )}
          {!loading && visibleMessages.length === 0 && (
            <div className="text-center text-xs text-slate-400">
              {activeContact.demo
                ? "Demo contact — select Bob to see real encrypted messages."
                : "No messages yet. Send one to start the conversation."}
            </div>
          )}
          {visibleMessages.map((message) => (
            <div
              key={message.id}
              className={`flex ${message.from === "me" ? "justify-end" : "justify-start"}`}
            >
              <div className={`max-w-[70%] ${message.from === "me" ? "text-right" : "text-left"}`}>
                <div
                  className={`inline-block rounded-2xl px-4 py-2.5 text-sm leading-relaxed shadow-sm ${
                    message.from === "me"
                      ? "rounded-br-sm bg-emerald-500 text-white"
                      : "rounded-bl-sm bg-white text-slate-700"
                  }`}
                >
                  {message.text}
                </div>
                <div className="mt-1 px-1 text-[10px] text-slate-400">
                  {message.time}
                  {message.txHash && (
                    <span className="ml-2 font-mono text-emerald-600" title={message.txHash}>
                      tx {message.txHash.slice(0, 10)}…
                    </span>
                  )}
                </div>
              </div>
            </div>
          ))}
        </div>

        {/* Composer */}
        <div className="border-t border-slate-200 bg-white px-6 py-4">
          <div className="flex items-end gap-3">
            <textarea
              value={draft}
              onChange={(e) => setDraft(e.target.value)}
              onKeyDown={(e) => {
                if (e.key === "Enter" && !e.shiftKey) {
                  e.preventDefault();
                  sendMessage();
                }
              }}
              rows={2}
              placeholder={`Message ${activeContact.name}...`}
              className="flex-1 resize-none rounded-xl border border-slate-200 bg-slate-50 px-4 py-2.5 text-sm outline-none focus:border-emerald-400 focus:bg-white"
            />
            <button
              onClick={sendMessage}
              disabled={sending}
              className="rounded-xl bg-emerald-500 px-6 py-3 text-sm font-medium text-white transition-colors hover:bg-emerald-600 disabled:bg-slate-300"
            >
              {sending ? "Sending…" : "Send"}
            </button>
          </div>
          {sendError && (
            <p className="mt-2 text-[11px] leading-relaxed text-red-600">{sendError}</p>
          )}
          <p className="mt-2 text-[11px] text-slate-400">
            Enter to send · plaintext is encrypted in Rust, only ciphertext goes on-chain
          </p>
        </div>
      </main>
    </div>
  );
}

export default App;
