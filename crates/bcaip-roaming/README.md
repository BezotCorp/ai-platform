# bcaip-roaming

Peer-to-peer transport for BCAIP agents, built on [iroh](https://iroh.computer) (QUIC, using iroh's public relays for NAT traversal).

It lets a BCAIP agent accept connections from a remote ACP client (another BCAIP instance, or any other ACP client) that drives it, and lets a client dial a remote agent to hold an interactive session, delegate a one-shot task, or bridge it to a local ACP client — typically without port-forwarding.

This crate is a **standalone library** with no dependency on BCAIP core, so the iroh dependency stays isolated from core. It knows nothing about agents or sessions, only identity, trust, and authenticated byte streams, and can be embedded in any Rust application.

The consumer surface is `RoamingNode` (bind/share/connect), `RoamingConfig`, the two-method `AcpStreamServer` trait used to plug in whatever serves a stream, `TrustBook`, and `ConnectionCard`.

See `examples/echo_roundtrip.rs` for the whole flow in one file:

```bash
cargo run -p bcaip-roaming --example echo_roundtrip
```

The code that bridges the transport to BCAIP's agent machinery lives in `bcaip-cli` behind the optional `roaming` feature; it isn't compiled unless that feature is enabled.

## The model: an authenticated ACP transport with mutual key trust

Roaming does one thing: provide an **authenticated peer-to-peer ACP transport**.

The host runs BCAIP's real ACP server; the connecting side is an ACP client. Everything "session-shaped" (list/load/new/prompt) is therefore plain ACP that happens to run over a roaming connection — roaming adds no session semantics.

Trust is a **mutual, public-key allowlist** — WireGuard / SSH-known-hosts style, not a capability token:

- Each node has **one** ed25519 identity that _is_ its iroh `EndpointId`. The QUIC-TLS handshake proves a peer holds the secret for the id it claims, so a key cannot be impersonated. It is persisted as hex in a `0600` file in the config directory.
- A node produces a **connection card** (`ConnectionCard`) — a non-secret string carrying its public key + relay URLs, plus a short fingerprint for out-of-band verification. It never expires and grants nothing on its own.
- You **swap cards** and each side **accepts** the other's key. A connection succeeds only if the host has accepted the dialer's key, and an accepted peer gets the host's full ACP surface. A leaked card lets no one in; there is no bearer token that works by possession.

## Concepts

- **`ConnectionCard`** — the shareable, non-secret identity + reachability string (`bcaip+roam://…`). Encodes public key + relay URLs and exposes `fingerprint()`.
- **`TrustBook`** — the local, mutual allowlist of accepted peer keys, plus revocations. Access exists _only_ by accepting a key. Persisted atomically and re-read on each inbound connection, so `accept` / `revoke` take effect against a running `share` without a restart. Reload failure fails **closed**.
- **`Directory`** — an out-of-band record of connections that actually happened (inbound and outbound), built purely from observed connections. No gossip.
- **`PeerBook`** — a user-managed address book of remotes, by nickname; stores the peer's non-secret card.

## Flow

```text
both:    bind endpoint ──▶ `roam id` prints a connection card ──▶ swap cards
host:    `roam peers accept <peer>` ──▶ `roam share` (serve to accepted keys)
client:  `roam peers add <card>` ──▶ dial via relay ──▶ handshake (label only)
host:    authorize by TLS-authenticated key ──▶ ACP serve() (full surface)
client:  run an ACP client over the same bi-stream
```

An iroh bidirectional stream is the byte transport for BCAIP's existing transport-agnostic ACP `serve` / `ByteStreams` seam, so hosting reuses the ACP server and the client reuses the ACP client.

## CLI

Exposed via `bcaip roam` in `bcaip-cli`, behind the `roaming` feature:

| Command                                                                    | Purpose                                         |
| -------------------------------------------------------------------------- | ----------------------------------------------- |
| `roam id` (alias `card`)                                                   | Print this node's connection card               |
| `roam peers add <card> [name]`                                             | Save a peer's card to the address book          |
| `roam peers accept <peer\|card> [name]`                                    | Accept inbound connections from a key           |
| `roam peers revoke <peer\|card\|id>`                                       | Stop accepting a key                            |
| `roam peers list`                                                          | Saved peers + which keys are accepted           |
| `roam share [--cwd] [--with-builtin]`                                      | Host this agent to accepted peers               |
| `roam connect <peer\|card>`                                                | Quick interactive REPL (debug/peek)             |
| `roam delegate <peer\|card> ["<task>"] [--session <id>] [--list-sessions]` | One-shot task, or list/continue remote sessions |
| `roam bridge <peer\|card> [--listen <addr>]`                               | Expose the remote agent as a local ACP endpoint |
| `roam connections`                                                         | Live/observed connections (no gossip)           |

## Testing across two disconnected machines

Build both with the roaming feature:

```bash
cargo build -p bcaip-cli --features roaming
```

No shared network, VPN, or port-forwarding is needed; the public n0 relays bridge them.

On **each** machine, run:

```bash
bcaip roam id
```

and send the printed `bcaip+roam://…` card to the other side out of band.

On **machine A** (the host):

```bash
bcaip roam peers accept '<B card>'
bcaip roam share
```

Optionally pass `--cwd <dir>`. It defaults to the directory where `share` was started, and the connector's own path is always ignored.

On **machine B**:

```bash
bcaip roam peers add '<A card>' boxA
```

Then either drive A interactively:

```bash
bcaip roam connect boxA
```

hand it a one-shot task:

```bash
bcaip roam delegate boxA "what is 2+2?"
```

or enumerate / continue A's sessions:

```bash
bcaip roam delegate boxA --list-sessions
bcaip roam delegate boxA --session <id> "<task>"
```

Verify that A is really doing the work by asking something machine-specific, such as its hostname and current working directory.

On the host:

```bash
bcaip roam connections
```

shows who connected.

If session creation hangs on macOS, prefix the command with:

```bash
BCAIP_DISABLE_KEYRING=1
```

The environment-variable name is retained for compatibility unless the corresponding configuration surface is renamed separately.

## Design decisions & rationale

**Roaming is just an ACP transport.**

The host runs the agent loop — its tools, working directory, and shell — while the connecting side is an ACP client.

Each connection gets a fresh agent driving its own sessions. `FullAcpBridge` hands the stream to BCAIP's real ACP `serve`.

`connect` is a thin ACP client UI, not a provider wrapper. Wrapping the remote as a provider for a second local agent loop would double the loop and defeat the point.

**The host controls the working directory.**

ACP's `session/new` carries a cwd, but the connector's absolute path is meaningless on the host machine.

The host therefore ignores the sent cwd and imposes its own — the directory `roam share` was started in, or the one supplied through `--cwd`. The client sends only a placeholder.

**Trust is mutual and key-based, with no bearer path.**

A card is non-secret and grants nothing. A share admits no one until a key is explicitly accepted, so the safe default — admit nobody — is built in.

Authorization uses the full TLS-authenticated key. The handshake carries only a display label, which is not trusted.

Acceptance is re-read for every connection and fails closed, so revocation takes effect against a running share.

**Acceptance is all-or-nothing.**

An accepted peer gets the host's full ACP surface. There is no per-request authorization gate and therefore no finer-grained role model.

Simultaneous multi-viewer co-driving of one live session is a possible future feature. It is not expressible over plain 1:1 ACP and is intentionally out of scope here.

**Delegation guardrails are about cost, not authorization.**

The peer is already trusted, so the concern with agent-to-agent delegation is runaway cost from loops such as A → B → A.

The `delegate` path auto-cancels tool-permission requests because there is no human present to answer them.

## What's deferred

- **Live multi-viewer co-driving** (paseo-style): several clients watching and steering _one_ in-flight session at once. This isn't expressible over plain 1:1 ACP. It needs a purpose-built multi-party session protocol — subscribe / snapshot / broadcast / steer with an explicit controller — layered over this transport. It is deliberately not emulated through an ACP broker.
- Self-hosted relays. Public n0 relays are rate-limited.

## Surfacing delegation to the model

The agent can reach other agents with **no new code**: a builtin skill (`roam-delegate`) documents how to call:

```bash
bcaip roam delegate <peer> "<task>"
```

through the shell.

It ships with the platform but is inert unless the `roaming` CLI feature is built in, keeping iroh out of core.

## Browser web client

The existing browser client for roaming lives in a separate upstream repository:

[BezotCorp/ai-platform-mobile](https://github.com/BezotCorp/ai-platform-mobile/tree/main/mobile-web)

Its `mobile-web/` application is a pure-browser React app that connects to a roaming host. iroh compiled to wasm runs _inside the browser tab_, driving the agent over ACP.

There is no Tauri application or local bridge; the tab itself is the roaming peer.

The stock iroh wasm build tunnels QUIC over WebSocket to the relay because its UDP transport is compiled out in browsers. A custom WebRTC transport could add direct paths later.

It is fully decoupled from this crate. The BCAIP roaming web wasm crate in that repository currently **mirrors** this crate's connection-card and frame wire format by copying its constants (`CARD_VERSION`, `MAX_FRAME_BYTES`, card bounds).

When the wire format changes here, the browser implementation must be updated in the same change. Drift will not fail to compile across repositories; it will break pairing at runtime.

The upstream names are retained here because they identify the actual external repository and crate.

## Prior art

Patterns here were informed by studying a sibling production project that runs iroh 1.0 for distributed LLM inference: minimal-preset endpoints with custom relay maps, ALPN-based stream dispatch, and reachability via relay routing by node id.

A connection card therefore needs only key + relay information, not a fixed address.
