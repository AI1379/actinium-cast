# Actinium Cast

A pure anonymous, censorship-resistant, and entirely decentralized content distribution platform built upon the BitTorrent Mainline DHT network and lightweight blockchain technologies.

## Architectural Overview

Actinium Cast abandons traditional server-client models. It uses a three-layer architecture to ensure permanent availability, extreme anonymity, and community-driven moderation.

### 1. P2P Storage & Mesh Network (Gateway)

- **BitTorrent DHT (BEP 44):** All posts, comments, and votes are signed via Ed25519 and injected directly into the global BitTorrent DHT as Mutable Items.
- **Tracker Discovery (BEP 5):** Gateways are stateless backend proxies. They use BEP 5 `announce_peer` to find each other globally and automatically form a decentralized Mesh network to sync index data via background polling. This ensures any single node can cold-boot and recover the entire ecosystem's state without knowing the full public key list in advance.

### 2. Client-Side Cryptography & Anti-Spam (Wasm/CLI)

- **Zero-Knowledge Architecture:** Gateways never store user private keys. All Ed25519 key generation and payload signing occur locally in the user's browser (via WebAssembly) or CLI.
- **Proof-of-Work (PoW):** To prevent malicious Sybil or spam attacks in a completely open network, frontends must commit to CPU-intensive Hashcash PoW computation before broadcasting. Gateways instantly reject non-compliant packets.

### 3. Consensus & Arbitration Layer (Blockchain)

- **Tombstone Mechanics:** Actinium uses an EVM-compatible Smart Contract strictly for threshold multi-signature governance. Core maintainers can propose and ratify the deletion of malicious content (CSAM, etc.).
- **Global Event Sync:** Once a quorum is reached, the contract emits a `GlobalBan` event that all Gateways listen to, instantly purging the toxic DHT hash locally.
- **Cost-Free Scalability:** Gateways connect agnostically to standard JSON-RPC endpoints. You can run the governance contract on Polygon (low-cost) or a completely private local PoA/Substrate chain (zero-cost).

## Current Development Status

- [x] Pure Rust core algorithm implementation
- [x] Hardened Ed25519 offline identity & signature verification
- [x] Hashcash PoW anti-spam mechanism
- [x] BitTorrent DHT BEP 44 bridging
- [x] Gateway Mesh Sync & Network discovery via BEP 5
- [ ] EVM Smart Contract threshold consensus
- [ ] Wasm Web Frontend

## Getting Started

Currently, testing is driven by our CLI client interacting with local Gateway nodes.

```bash
# 1. Start a Gateway Node (binds to 3000 by default)
cargo run -p gateway

# 2. Open another terminal and generate a new offline Identity
cargo run -p client-cli -- identity generate

# 3. Post anonymously (Client calculates PoW locally and asks the Gateway to route it)
cargo run -p client-cli -- post -s <SECRET_KEY> -d 0 -t "Hello Decentralized World" -c "Content"
```

> **Local Mesh Testing**: You can run multiple gateways locally by altering the ports and enabling the local Mesh bypass flag (`ACTINIUM_DEV_LOCAL_MESH=1`).

Please refer to [`docs/ARCHITECTURE.md`](docs/ARCHITECTURE.md) and [`docs/ROADMAP.md`](docs/ROADMAP.md) for detailed design blueprints and targeted milestones.
