# BitcoinWolfe

**A modern Bitcoin full node built in Rust, powered by libbitcoinkernel for byte-for-byte consensus compatibility with Bitcoin Core.**

BitcoinWolfe pairs Bitcoin Core's own consensus engine (extracted as a C library) with a clean Rust
stack for networking, storage, mempool policy, wallet, Lightning, and Nostr functionality. The result
is a node that validates exactly like Core but is built from the ground up with modern Rust tooling.

> **Note:** BitcoinWolfe is open-source software in active development. Use at your own risk. Contributions, bug reports, and feedback are welcome.

---

## Architecture

```
                         +------------------+
                         |    wolfe-node    |
                         |   (CLI binary)   |
                         +--------+---------+
                                  |
     +--------+--------+---------+---------+---------+--------+
     |        |        |         |         |         |        |
 +---+----+ +-+------+ +-+------+ +------+-+ +------+-+ +---+-----+
 | wolfe- | | wolfe- | | wolfe- | | wolfe- | | wolfe- | | wolfe-  |
 | consen | | p2p    | | mempl  | | wallet | | lightn | | nostr   |
 | (kern) | | (tokio)| | (pol.) | | (BDK)  | | (LDK)  | | (sdk)   |
 +---+----+ +---+----+ +---+----+ +---+----+ +---+----+ +---+-----+
     |          |           |          |          |           |
     +----+-----+-----------+----------+----------+-----------+
          |                 |                     |
  +-------+-------+ +------+------+       +------+------+
  | wolfe-store   | | wolfe-types |       | wolfe-rpc   |
  | (redb)        | | (config)    |       | (axum)      |
  +--------------+  +-------------+       +-------------+
```

| Crate | Purpose |
|---|---|
| `wolfe-consensus` | libbitcoinkernel FFI via the `bitcoinkernel` 0.2 crate -- consensus validation identical to Bitcoin Core |
| `wolfe-store` | Block headers, peers, and node metadata stored in redb (pure Rust, ACID, zero-copy reads) |
| `wolfe-p2p` | Async P2P networking on Tokio with Bitcoin protocol message serialization (rust-bitcoin 0.32), BIP324 v2 transport |
| `wolfe-mempool` | Configurable policy engine (OP_RETURN limits, fee floors, RBF, ancestor/descendant limits) + concurrent transaction pool |
| `wolfe-wallet` | BDK 1.1 descriptor wallet with SQLite persistence, PSBT support, and coin selection |
| `wolfe-lightning` | Lightning Network integration via LDK 0.2 -- peer connections, channels, invoices, payments |
| `wolfe-nostr` | Nostr integration via nostr-sdk -- block announcements, fee oracle, NIP-98 RPC auth |
| `wolfe-rpc` | axum REST API + Bitcoin Core-compatible JSON-RPC server with Prometheus metrics |
| `wolfe-types` | TOML configuration, shared types, and error definitions |
| `wolfe-node` | Main binary (`wolfe`) that wires everything together |

---

## Quick Start

### Prerequisites

- **Rust 1.85+** (edition 2021)
- **cmake** and a **C++ compiler** (required to build libbitcoinkernel)

### Build

```bash
git clone https://github.com/refined-element/BitcoinWolfe.git
cd BitcoinWolfe
cargo build --release
```

The binary is at `target/release/wolfe`.

### Run

```bash
# Start the node with default settings (mainnet)
./target/release/wolfe start

# Start on a different network
./target/release/wolfe --network signet start

# Use a custom config file
./target/release/wolfe --config my-config.toml start
```

### Stop

The node shuts down gracefully via any of these methods:

```bash
# From the terminal that started it
Ctrl+C

# Via JSON-RPC
curl -s http://127.0.0.1:8332/ \
  -H "Content-Type: application/json" \
  -d '{"jsonrpc":"2.0","id":1,"method":"stop"}'

# Via signal (SIGINT or SIGTERM)
kill -INT $(pgrep -f "wolfe start")   # or: kill $(pgrep -f "wolfe start")
```

You can also stop from the **Settings** page in the web dashboard.

All methods trigger the same graceful shutdown: Lightning channel state is persisted, the consensus engine is interrupted cleanly, and peer connections are closed. Both `SIGINT` (Ctrl+C) and `SIGTERM` (default for `kill`, `systemctl stop`, and `launchctl unload`) are handled the same way.

### Auto-Start on macOS (launchd)

To have BitcoinWolfe start automatically on login and restart after crashes:

```bash
# 1. Copy the template plist
cp config/com.bitcoinwolfe.node.plist ~/Library/LaunchAgents/

# 2. Edit it — replace the placeholder paths with your actual paths:
#    __WOLFE_BINARY__ → full path to the wolfe binary (e.g. /Users/you/BitcoinWolfe/target/release/wolfe)
#    __WOLFE_DIR__    → full path to the project directory (e.g. /Users/you/BitcoinWolfe)
nano ~/Library/LaunchAgents/com.bitcoinwolfe.node.plist

# 3. Load the service
launchctl load ~/Library/LaunchAgents/com.bitcoinwolfe.node.plist

# 4. Verify it's running
launchctl list | grep bitcoinwolfe
curl -s http://127.0.0.1:8332/api/peers | head -c 100
```

To manage the service:

```bash
# Stop the service (and prevent auto-restart)
launchctl unload ~/Library/LaunchAgents/com.bitcoinwolfe.node.plist

# Reload after config changes
launchctl unload ~/Library/LaunchAgents/com.bitcoinwolfe.node.plist
launchctl load ~/Library/LaunchAgents/com.bitcoinwolfe.node.plist
```

> **Note:** The plist uses `KeepAlive` with `SuccessfulExit = false`, so launchd will restart the node if it crashes but not if you stop it intentionally via RPC `stop`, `Ctrl+C`, or the dashboard.
>
> `~/Library/LaunchAgents/` runs at user **login**, not at boot. For boot-time startup install the plist into `/Library/LaunchDaemons/` (requires `sudo`) and add a `UserName` key — note that `LaunchDaemons` run as root unless `UserName` is set.

### Auto-Start on Linux (systemd)

```bash
# Create a service file
sudo tee /etc/systemd/system/bitcoinwolfe.service << 'EOF'
[Unit]
Description=BitcoinWolfe Node
After=network-online.target
Wants=network-online.target

[Service]
Type=simple
User=YOUR_USER
WorkingDirectory=/path/to/BitcoinWolfe
ExecStart=/path/to/BitcoinWolfe/target/release/wolfe start
Restart=on-failure
RestartSec=10
# systemctl stop sends SIGTERM, which the node handles gracefully.
# Give Lightning state and consensus enough time to persist before SIGKILL.
TimeoutStopSec=30

[Install]
WantedBy=multi-user.target
EOF

# Enable and start
sudo systemctl daemon-reload
sudo systemctl enable bitcoinwolfe
sudo systemctl start bitcoinwolfe

# Check status
sudo systemctl status bitcoinwolfe
```

---

## CLI Usage

```
wolfe [OPTIONS] [COMMAND]

Commands:
  start           Start the node (default if no command given)
  default-config  Print the default TOML configuration to stdout
  info            Print version and build info

Options:
  -c, --config <PATH>     Path to config file [default: wolfe.toml]
  -n, --network <NETWORK> Override the Bitcoin network (mainnet, testnet, signet, regtest)
  -d, --datadir <PATH>    Override the data directory
```

```bash
# Dump a starter config, then customize it
wolfe default-config > wolfe.toml

# Check the build
wolfe info
```

---

## Configuration

BitcoinWolfe is configured via TOML. Run `wolfe default-config` to generate a starting point.

### P2P

```toml
[p2p]
listen = "0.0.0.0:8333"
max_inbound = 125
max_outbound = 10
prefer_v2_transport = true     # BIP324 encrypted transport
dns_seeds = []                 # Empty = built-in defaults
connect = []                   # Manual peers (bypasses DNS)
ban_duration_secs = 86400
```

### Mempool Policy

```toml
[mempool]
max_size_mb = 300
min_fee_rate = 1.0             # sat/vB
max_datacarrier_bytes = 80     # OP_RETURN limit
datacarrier = true             # Accept OP_RETURN outputs
full_rbf = true                # Full Replace-By-Fee
max_ancestors = 25
max_descendants = 25
expiry_hours = 336
```

### RPC / REST API

```toml
[rpc]
enabled = true
listen = "127.0.0.1:8332"
rest_enabled = true
user = "rpcuser"               # HTTP Basic auth
password = "rpcpassword"
cors_origins = []
```

### Wallet

```toml
[wallet]
enabled = false                # Opt-in
db_path = "wallet.sqlite3"
```

### Lightning

```toml
[lightning]
enabled = false                # Opt-in
listen_port = 9735
alias = "BitcoinWolfe"
color = "ff9900"
accept_inbound_channels = true
min_channel_size_sat = 20000
max_channel_size_sat = 16777215
# rapid_gossip_sync_url = "https://rapidsync.lightningdevkit.org/snapshot"

# Lightning peers that the node should keep connected. The reconnector
# checks every 60 seconds and dials any listed peer that's currently
# offline. Hostnames are re-resolved on each tick.
# persistent_peers = ["02abc...@host.example.com:9735"]
```

### Nostr

```toml
[nostr]
enabled = false
relays = ["wss://relay.damus.io", "wss://nos.lol"]
block_announcements = true
fee_oracle = true
fee_oracle_interval_secs = 60
nip98_auth = false             # NIP-98 Nostr auth for RPC
# allowed_pubkeys = ["npub1..."]
```

### Storage and Metrics

```toml
[storage]
prune_target_mb = 0            # 0 = full node, no pruning
db_cache_mb = 450

[metrics]
enabled = true
listen = "127.0.0.1:9332"     # Prometheus scrape endpoint
```

---

## API

### REST Endpoints

| Method | Path | Description |
|---|---|---|
| GET | `/api/info` | Node version, chain, block height, mempool size, uptime |
| GET | `/api/blockchain` | Chain tip height, best block hash, sync status |
| GET | `/api/mempool` | Mempool size, byte count, active policy settings |
| GET | `/api/peers` | Connected peers with user agents, versions, transport info |
| GET | `/api/lightning/info` | Lightning node ID, channel count, peer count |
| GET | `/api/lightning/channels` | Lightning channel list with capacities and status |
| GET | `/api/lightning/payments` | Lightning payment history (send/receive/failed) |

### JSON-RPC (Bitcoin Core Compatible)

POST to `/` with standard JSON-RPC 2.0 payloads.

#### Blockchain

| Method | Description |
|---|---|
| `getblockchaininfo` | Chain, block height, best block hash, IBD status |
| `getblockcount` | Current block height |
| `getbestblockhash` | Hash of the chain tip |
| `getblock` | Block data by hash (raw hex or parsed, by verbosity) |

#### Network

| Method | Description |
|---|---|
| `getnetworkinfo` | Version, user agent, protocol version, connection count |
| `getpeerinfo` | Per-peer address, user agent, version, direction |

#### Mempool

| Method | Description |
|---|---|
| `getmempoolinfo` | Mempool loaded status, size, bytes, minimum fee |
| `getrawmempool` | List of transaction IDs in the mempool |
| `getrawtransaction` | Raw transaction data by txid (mempool lookup) |
| `sendrawtransaction` | Submit a raw transaction to the mempool |

#### Wallet

| Method | Description |
|---|---|
| `getbalance` | Confirmed wallet balance in BTC |
| `getwalletinfo` | Balance breakdown (confirmed, unconfirmed, immature) |
| `getnewaddress` | Generate a new receiving address |
| `listtransactions` | List wallet transactions |
| `walletcreatefundedpsbt` | Create a funded PSBT for sending |
| `walletprocesspsbt` | Sign a PSBT with the wallet's keys |

#### Lightning

| Method | Params | Description |
|---|---|---|
| `ln_getinfo` | | Node ID, channel/peer counts |
| `ln_listchannels` | | Channel list with capacities and status |
| `ln_listpeers` | | Connected Lightning peers |
| `ln_connect` | `["pubkey@host:port"]` | Connect to a Lightning peer |
| `ln_openchannel` | `["pubkey", amount_sat, push_msat?]` | Open a channel |
| `ln_invoice` | `[amount_msat?, description?, expiry_secs?]` | Create a BOLT11 invoice |
| `ln_pay` | `["lnbc..."]` | Pay a BOLT11 invoice |
| `ln_listpayments` | `[limit?]` | Payment history (default 50, most recent first) |

#### Node

| Method | Description |
|---|---|
| `uptime` | Node uptime in seconds |
| `stop` | Initiate graceful shutdown |

### Examples

```bash
# Get blockchain info
curl -s http://127.0.0.1:8332/ \
  -H "Content-Type: application/json" \
  -d '{"jsonrpc":"2.0","id":1,"method":"getblockchaininfo"}' | jq

# Connect to a Lightning peer
curl -s http://127.0.0.1:8332/ \
  -H "Content-Type: application/json" \
  -d '{"jsonrpc":"2.0","id":1,"method":"ln_connect","params":["02abc...@host:9735"]}' | jq

# Create a Lightning invoice for 50 sats
curl -s http://127.0.0.1:8332/ \
  -H "Content-Type: application/json" \
  -d '{"jsonrpc":"2.0","id":1,"method":"ln_invoice","params":[50000,"coffee"]}' | jq
```

---

## Dashboard

BitcoinWolfe includes a web dashboard embedded directly in the binary. No separate server or setup required.

### Usage

Just start the node and open the RPC address in your browser:

```
http://127.0.0.1:8332
```

The dashboard is served from the same port as the JSON-RPC and REST APIs. `POST /` still works for JSON-RPC, while `GET /` serves the dashboard.

### Pages

| Page | What it shows |
|---|---|
| **Overview** | Block height, sync progress, peer count, mempool, uptime |
| **Wallet** | Balance, addresses, send/receive, transaction history |
| **Lightning** | Channels, capacity bars, payment history, create invoices, pay invoices |
| **Peers** | Connected Bitcoin P2P peers with user agents and versions |
| **Nostr** | Relay connections, published events, npub |
| **Settings** | Node URL, auth credentials, poll interval |

### Development

```bash
cd dashboard
npm run dev          # Vite dev server with hot reload (port 5173)
npm run test         # Run Vitest test suite
npm run build        # Production build to dashboard/build/
```

---

## Roadmap

### Done

- [x] Workspace structure with 10 crates
- [x] libbitcoinkernel integration for consensus validation
- [x] redb-based persistent storage (headers, metadata, peers)
- [x] P2P version handshake and message serialization
- [x] BIP324 v2 encrypted transport support
- [x] Header-first initial block download (IBD)
- [x] Full block download and validation
- [x] Block pruning (configurable target size)
- [x] Mempool with configurable policy engine (fees, OP_RETURN, RBF, ancestor limits)
- [x] BDK descriptor wallet with SQLite backend
- [x] PSBT workflow (create, sign)
- [x] REST API and Bitcoin Core-compatible JSON-RPC
- [x] RPC authentication (HTTP Basic + NIP-98 Nostr auth)
- [x] Lightning Network via LDK (peer connect, channels, invoices, payments)
- [x] Nostr integration (block announcements, fee oracle)
- [x] TOML configuration with CLI overrides
- [x] Prometheus metrics endpoint
- [x] Structured logging (text and JSON formats)
- [x] Graceful shutdown handling

- [x] Web dashboard (SvelteKit) with live node monitoring
- [x] Lightning payment history with persistence
- [x] Transaction broadcast via `sendrawtransaction` RPC
- [x] L402 Lightning-gated API endpoints

- [x] Embedded dashboard in the `wolfe` binary (zero-setup web UI)

### Planned

- [ ] Compact block relay (BIP152)
- [ ] Lightning BOLT12 offers
- [ ] Rapid Gossip Sync
- [ ] Configuration hot-reload
- [ ] Peer scoring and eviction logic

---

## Credits

BitcoinWolfe builds on the work of several excellent projects:

- **[libbitcoinkernel](https://github.com/bitcoin/bitcoin/tree/master/src/kernel)** -- Bitcoin Core's consensus engine, extracted as a standalone C library.
- **[rust-bitcoin](https://github.com/rust-bitcoin/rust-bitcoin)** (0.32) -- Bitcoin data structures, script types, and protocol serialization.
- **[BDK](https://bitcoindevkit.org/)** (1.1) -- Descriptor-based wallet with coin selection, PSBT, and SQLite persistence.
- **[LDK](https://lightningdevkit.org/)** (0.2) -- Lightning Development Kit for channel management, routing, and payments.
- **[nostr-sdk](https://github.com/rust-nostr/nostr)** (0.39) -- Nostr protocol client for decentralized event publishing.
- **[redb](https://github.com/cberner/redb)** -- Pure Rust, ACID-compliant, zero-copy embedded database.
- **[Tokio](https://tokio.rs/)** -- Async runtime powering the P2P, Lightning, and RPC layers.
- **[axum](https://github.com/tokio-rs/axum)** -- HTTP framework for the REST and JSON-RPC server.

---

## License

MIT -- see [LICENSE](LICENSE).
