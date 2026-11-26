import { useEffect, useRef, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import "./App.css";

// ---------------------------------------------------------------------------
// Security boundary:
// The private key is typed into the login gate, sent to Rust exactly once via
// `unlock`, and cleared from React state immediately. It is never persisted
// anywhere. Only PUBLIC data is cached locally: contacts (address + pubkey).
// ---------------------------------------------------------------------------

const CONTACTS_KEY = "crypto-chat.contacts.v1";

type Identity = {
  address: string;
  pubkey: string;
};

type Registration = {
  status: "registered" | "already";
  tx_hash: string | null;
};

type Contact = {
  id: string;
  name: string;
  address: string;
  fullAddress: string;
  pubkey: string;
  source: "discovered" | "manual";
};

type Message = {
  id: number;
  contactId: string;
  from: "me" | "them";
  text: string;
  time: string;
  txHash?: string;
};

type FetchedMessage = {
  from: "me" | "them";
  text: string;
  tx_hash: string;
  timestamp: number;
};

type DiscoveredContact = {
  address: string;
  pubkey: string;
};

function shortAddress(address: string): string {
  if (address.length < 16) return address;
  return `${address.slice(0, 6)}…${address.slice(-4)}`;
}

function loadCachedContacts(): Contact[] {
  try {
    const raw = localStorage.getItem(CONTACTS_KEY);
    if (!raw) return [];
    const parsed = JSON.parse(raw) as Contact[];
    return Array.isArray(parsed) ? parsed : [];
  } catch {
    return [];
  }
}

function LoginGate({ onUnlocked }: { onUnlocked: (id: Identity) => void }) {
  const [key, setKey] = useState("");
  const [error, setError] = useState("");
  const [busy, setBusy] = useState(false);

  async function unlock() {
    const value = key.trim();
    if (!value || busy) return;
    setBusy(true);
    setError("");
    try {
      const id = await invoke<Identity>("unlock", { privateKey: value });
      setKey("");
      onUnlocked(id);
    } catch (e) {
      setError(String(e));
    } finally {
      setBusy(false);
    }
  }

  return (
    <div className="flex h-screen w-screen items-center justify-center bg-slate-100">
      <div className="w-full max-w-md rounded-2xl border border-slate-200 bg-white p-8 shadow-sm">
        <h1 className="text-2xl font-semibold tracking-tight text-slate-800">crypto-chat</h1>
        <p className="mt-1 text-xs text-slate-400">E2EE over Ethereum · Sepolia</p>

        <label className="mt-8 block text-sm font-medium text-slate-700">Private key</label>
        <input
          type="password"
          value={key}
          onChange={(e) => setKey(e.target.value)}
          onKeyDown={(e) => {
            if (e.key === "Enter") {
              e.preventDefault();
              unlock();
            }
          }}
          placeholder="0x…"
          autoComplete="off"
          spellCheck={false}
          className="mt-2 w-full rounded-xl border border-slate-200 bg-slate-50 px-4 py-3 font-mono text-sm outline-none focus:border-emerald-400 focus:bg-white"
        />

        {error && <p className="mt-3 text-xs leading-relaxed text-red-600">{error}</p>}

        <button
          onClick={unlock}
          disabled={busy || !key.trim()}
          className="mt-6 w-full rounded-xl bg-emerald-500 px-6 py-3 text-sm font-medium text-white transition-colors hover:bg-emerald-600 disabled:bg-slate-300"
        >
          {busy ? "Unlocking…" : "Unlock"}
        </button>

        <div className="mt-6 rounded-lg bg-slate-50 p-3 text-[11px] leading-relaxed text-slate-500">
          The key is sent once to the Rust layer and cleared from here. It is never
          written to disk or sent to the network. Only ciphertext reaches Sepolia.
        </div>
      </div>
    </div>
  );
}

function ChatScreen({
  identity,
  onLock,
}: {
  identity: Identity;
  onLock: () => void;
}) {
  const [contacts, setContacts] = useState<Contact[]>(loadCachedContacts);
  const [selectedId, setSelectedId] = useState<string>("");
  const [messages, setMessages] = useState<Message[]>([]);
  const [draft, setDraft] = useState("");
  const [sending, setSending] = useState(false);
  const [sendError, setSendError] = useState("");
  const [loading, setLoading] = useState(false);
  const [registryStatus, setRegistryStatus] = useState("Registering your public key…");
  const [newAddress, setNewAddress] = useState("");
  const [addError, setAddError] = useState("");
  const [adding, setAdding] = useState(false);
  const bootstrapped = useRef(false);

  // Persist only public contact data (never keys).
  useEffect(() => {
    localStorage.setItem(CONTACTS_KEY, JSON.stringify(contacts));
  }, [contacts]);

  // One-time startup: publish our key, then discover peers from chain logs.
  useEffect(() => {
    if (bootstrapped.current) return;
    bootstrapped.current = true;

    (async () => {
      try {
        const result = await invoke<Registration>("register_pubkey");
        setRegistryStatus(
          result.status === "already" || !result.tx_hash
            ? "Public key already on-chain"
            : `Public key registered · tx ${result.tx_hash.slice(0, 10)}…`
        );
      } catch (e) {
        setRegistryStatus(`Registry unavailable: ${String(e)}`);
      }

      try {
        const found = await invoke<DiscoveredContact[]>("discover_contacts");
        setContacts((prev) => {
          const seen = new Set(prev.map((c) => c.fullAddress.toLowerCase()));
          const additions: Contact[] = found
            .filter((c) => !seen.has(c.address.toLowerCase()))
            .map((c) => ({
              id: c.address.toLowerCase(),
              name: shortAddress(c.address),
              address: shortAddress(c.address),
              fullAddress: c.address,
              pubkey: c.pubkey,
              source: "discovered" as const,
            }));
          return [...prev, ...additions];
        });
      } catch (e) {
        setSendError("Contact discovery failed: " + String(e));
      }
    })();
  }, []);

  const activeContact = contacts.find((c) => c.id === selectedId) ?? contacts[0];

  async function loadMessages(contact: Contact) {
    setLoading(true);
    setSendError("");
    try {
      const fetched = await invoke<FetchedMessage[]>("fetch_messages", {
        peerAddress: contact.fullAddress,
        peerPubkey: contact.pubkey,
      });
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

  useEffect(() => {
    if (activeContact) loadMessages(activeContact);
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [selectedId, activeContact?.id]);

  async function sendMessage() {
    const text = draft.trim();
    if (!text || sending || !activeContact) return;
    setSending(true);
    setSendError("");
    try {
      await invoke<string>("send_message", {
        recipient: activeContact.fullAddress,
        peerPubkey: activeContact.pubkey,
        message: text,
      });
      setDraft("");
      await loadMessages(activeContact);
    } catch (e) {
      setSendError(String(e));
    } finally {
      setSending(false);
    }
  }

  async function addContact() {
    const address = newAddress.trim();
    if (!address || adding) return;
    setAdding(true);
    setAddError("");
    try {
      const pubkey = await invoke<string>("lookup_pubkey", { address });
      const id = address.toLowerCase();
      setContacts((prev) => {
        if (prev.some((c) => c.fullAddress.toLowerCase() === id)) return prev;
        return [
          ...prev,
          {
            id,
            name: shortAddress(address),
            address: shortAddress(address),
            fullAddress: address,
            pubkey: `0x04${pubkey.replace(/^0x/, "")}`,
            source: "manual",
          },
        ];
      });
      setNewAddress("");
      setSelectedId(id);
    } catch (e) {
      setAddError(String(e));
    } finally {
      setAdding(false);
    }
  }

  const visibleMessages = messages.filter((m) => m.contactId === activeContact?.id);

  return (
    <div className="flex h-screen w-screen overflow-hidden bg-slate-100 text-slate-800">
      <aside className="flex w-72 shrink-0 flex-col border-r border-slate-200 bg-white">
        <div className="border-b border-slate-200 px-5 py-4">
          <h1 className="text-lg font-semibold tracking-tight">crypto-chat</h1>
          <p className="mt-0.5 text-xs text-slate-400">E2EE over Ethereum · Sepolia</p>
        </div>

        <div className="border-b border-slate-200 px-3 py-3">
          <div className="flex gap-2">
            <input
              value={newAddress}
              onChange={(e) => setNewAddress(e.target.value)}
              onKeyDown={(e) => {
                if (e.key === "Enter") {
                  e.preventDefault();
                  addContact();
                }
              }}
              placeholder="Add contact by address"
              spellCheck={false}
              className="min-w-0 flex-1 rounded-lg border border-slate-200 bg-slate-50 px-2 py-1.5 font-mono text-[11px] outline-none focus:border-emerald-400"
            />
            <button
              onClick={addContact}
              disabled={adding || !newAddress.trim()}
              className="rounded-lg bg-emerald-500 px-3 text-xs font-medium text-white hover:bg-emerald-600 disabled:bg-slate-300"
            >
              {adding ? "…" : "Add"}
            </button>
          </div>
          {addError && <p className="mt-1.5 text-[10px] leading-relaxed text-red-600">{addError}</p>}
          <p className="mt-1.5 text-[10px] text-slate-400">
            Peer must have registered a public key on-chain.
          </p>
        </div>

        <div className="px-4 py-3 text-[11px] font-medium uppercase tracking-wide text-slate-400">
          Contacts
        </div>

        <div className="flex-1 overflow-y-auto">
          {contacts.length === 0 && <p className="px-4 text-xs text-slate-400">No contacts yet.</p>}
          {contacts.map((contact) => (
            <button
              key={contact.id}
              onClick={() => {
                setSelectedId(contact.id);
                setSendError("");
              }}
              className={`flex w-full items-center gap-3 px-4 py-3 text-left transition-colors ${
                contact.id === activeContact?.id ? "bg-emerald-50" : "hover:bg-slate-50"
              }`}
            >
              <span className="flex h-9 w-9 shrink-0 items-center justify-center rounded-full bg-emerald-500 text-xs font-semibold text-white">
                {contact.name[0].toUpperCase()}
              </span>
              <span className="min-w-0 flex-1">
                <span className="block truncate text-sm font-medium">{contact.name}</span>
                <span className="block truncate text-[10px] text-slate-400">
                  {contact.source} · {shortAddress(contact.fullAddress)}
                </span>
              </span>
            </button>
          ))}
        </div>

        <div className="border-t border-slate-200 px-4 py-3">
          <div className="text-[10px] font-medium uppercase tracking-wide text-slate-400">
            Signed in as
          </div>
          <div className="mt-1 font-mono text-[11px] text-slate-600">
            {shortAddress(identity.address)}
          </div>
          <p className="mt-2 text-[10px] leading-relaxed text-emerald-700">{registryStatus}</p>
          <details className="mt-2">
            <summary className="cursor-pointer text-[11px] text-emerald-700">
              Show my public key
            </summary>
            <div className="mt-2 break-all rounded-lg bg-slate-50 p-2 font-mono text-[10px] leading-relaxed text-slate-600">
              {identity.pubkey}
            </div>
          </details>
          <button
            onClick={onLock}
            className="mt-3 w-full rounded-lg border border-slate-200 px-3 py-2 text-xs font-medium text-slate-600 transition-colors hover:bg-slate-50"
          >
            Lock (erase key from memory)
          </button>
        </div>
      </aside>

      <main className="flex min-w-0 flex-1 flex-col">
        <header className="flex items-center gap-3 border-b border-slate-200 bg-white px-6 py-4">
          {activeContact ? (
            <>
              <span className="flex h-9 w-9 items-center justify-center rounded-full bg-emerald-500 text-sm font-semibold text-white">
                {activeContact.name[0].toUpperCase()}
              </span>
              <div>
                <div className="font-semibold">{activeContact.name}</div>
                <div className="font-mono text-xs text-slate-400">
                  {shortAddress(activeContact.fullAddress)}
                </div>
              </div>
              <span className="ml-auto rounded-full bg-emerald-100 px-3 py-1 text-xs text-emerald-700">
                E2EE ready
              </span>
            </>
          ) : (
            <div className="text-sm text-slate-400">Select or add a contact to start chatting</div>
          )}
        </header>

        <div className="flex-1 space-y-4 overflow-y-auto bg-slate-50 px-6 py-6">
          {loading && (
            <div className="text-center text-xs text-slate-400">Loading messages from chain…</div>
          )}
          {!loading && activeContact && visibleMessages.length === 0 && (
            <div className="text-center text-xs text-slate-400">
              No messages yet. Send one to start the conversation.
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
              placeholder={activeContact ? `Message ${activeContact.name}…` : "No contact selected"}
              disabled={!activeContact}
              className="flex-1 resize-none rounded-xl border border-slate-200 bg-slate-50 px-4 py-2.5 text-sm outline-none focus:border-emerald-400 focus:bg-white disabled:bg-slate-100"
            />
            <button
              onClick={sendMessage}
              disabled={sending || !activeContact}
              className="rounded-xl bg-emerald-500 px-6 py-3 text-sm font-medium text-white transition-colors hover:bg-emerald-600 disabled:bg-slate-300"
            >
              {sending ? "Sending…" : "Send"}
            </button>
          </div>
          {sendError && (
            <p className="mt-2 text-[11px] leading-relaxed text-red-600">{sendError}</p>
          )}
          <p className="mt-2 text-[11px] text-slate-400">
            Enter to send · encrypted in Rust · only ciphertext goes on-chain
          </p>
        </div>
      </main>
    </div>
  );
}

function App() {
  const [identity, setIdentity] = useState<Identity | null>(null);
  const [checking, setChecking] = useState(true);

  useEffect(() => {
    invoke<boolean>("is_unlocked")
      .then(async (unlocked) => {
        if (unlocked) setIdentity(await invoke<Identity>("identity"));
      })
      .catch(() => undefined)
      .finally(() => setChecking(false));
  }, []);

  async function lock() {
    await invoke("lock");
    setIdentity(null);
  }

  if (checking) {
    return (
      <div className="flex h-screen w-screen items-center justify-center bg-slate-100 text-sm text-slate-400">
        Loading…
      </div>
    );
  }
  if (!identity) return <LoginGate onUnlocked={setIdentity} />;
  return <ChatScreen identity={identity} onLock={lock} />;
}

export default App;
