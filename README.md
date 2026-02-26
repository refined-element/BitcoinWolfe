# BitcoinWolfe

**A modern Bitcoin full node built in Rust, powered by libbitcoinkernel for byte-for-byte consensus compatibility with Bitcoin Core.**

BitcoinWolfe pairs Bitcoin Core's own consensus engine (extracted as a C library) with a clean Rust
stack for networking, storage, mempool policy, and wallet functionality. The result is a node that
validates exactly like Core but is built from the ground up with modern Rust tooling.

---

## Architecture

```
                         +------------------+
                         |    wolfe-node    |
                         |   (CLI binary)   |
                         +--------+---------+
                                  |
          +-----------+-----------+-----------+-----------+
          |           |           |           |           |
   +------+------+  ++---------+ ++---------+ +---------++ +---------+
   | wolfe-      |  | wolfe-   | | wolfe-   | | wolfe-   | | wolfe-  |
   | consensus   |  | p2p      | | mempool  | | wallet   | | rpc     |
   | (kernel FFI)|  | (tokio)  | | (policy) | | (BDK)    | | (axum)  |
   +------+------+  +----+-----+ +----+-----+ +----+----+ +----+----+
          |               |           |             |           |
          +-------+-------+-----------+-------------+-----------+
                  |                   |
          +-------+-------+   +------+------+
          | wolfe-store   |   | wolfe-types |
          | (redb)        |   | (config)    |
          +--------------+   +-------------+
```

| Crate | Purpose |
|---|---|
| `wolfe-consensus` | libbitcoinkernel FFI via the `bitcoinkernel` 0.2 crate -- consensus validation identical to Bitcoin Core |
| `wolfe-store` | Block headers, peers, and node metadata stored in redb (pure Rust, ACID, zero-copy reads) |
| `wolfe-p2p` | Async P2P networking on Tokio with Bitcoin protocol message serialization (rust-bitcoin 0.32) |
| `wolfe-mempool` | Configurable policy engine (OP_RETURN limits, fee floors, RBF, ancestor/descendant limits) + concurrent transaction pool |
| `wolfe-wallet` | BDK 1.1 descriptor wallet with SQLite persistence, PSBT support, and coin selection |
| `wolfe-rpc` | axum REST API + Bitcoin Core-compatible JSON-RPC server |
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
# BitcoinWolfe v0.1.0
# Architecture:
#   Consensus:  libbitcoinkernel (Bitcoin Core kernel)
#   Wallet:     BDK (Bitcoin Dev Kit) with SQLite
#   Storage:    redb (pure Rust ACID key-value store)
#   P2P:        Tokio async with BIP324 support
#   API:        REST + JSON-RPC (Prometheus-native)
```

---

## Configuration

BitcoinWolfe is configured via TOML. Run `wolfe default-config` to generate a starting point.
Key sections and defaults:

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
cors_origins = []
```

### Wallet

```toml
[wallet]
enabled = false                # Opt-in
db_path = "wallet.sqlite3"
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

### JSON-RPC (Bitcoin Core Compatible)

POST to `/` with standard JSON-RPC 2.0 payloads. Supported methods:

| Method | Description |
|---|---|
| `getblockchaininfo` | Chain, block height, best block hash, IBD status |
| `getnetworkinfo` | Version, user agent, protocol version, connection count |
| `getmempoolinfo` | Mempool loaded status, size, bytes, minimum fee |
| `getpeerinfo` | Per-peer address, user agent, version, direction |
| `uptime` | Node uptime in seconds |
| `stop` | Initiate graceful shutdown |

```bash
curl -s -X POST http://127.0.0.1:8332/ \
  -H "Content-Type: application/json" \
  -d '{"jsonrpc":"2.0","id":1,"method":"getblockchaininfo"}' | jq
```

---

## Roadmap

### Done

- [x] Workspace structure with 8 crates
- [x] libbitcoinkernel integration for consensus validation
- [x] redb-based persistent storage
- [x] P2P version handshake and message serialization
- [x] Mempool with configurable policy engine (fees, OP_RETURN, RBF, ancestor limits)
- [x] BDK descriptor wallet with SQLite backend
- [x] REST API and Bitcoin Core-compatible JSON-RPC
- [x] TOML configuration with CLI overrides
- [x] Prometheus metrics endpoint
- [x] Structured logging (text and JSON formats)
- [x] Graceful shutdown handling

### Planned

- [ ] Full initial block download (IBD) over P2P
- [ ] BIP324 encrypted transport (v2)
- [ ] Block relay and compact block relay (BIP152)
- [ ] Transaction relay and fee estimation
- [ ] Wallet `apply_block()` chain feeding from node
- [ ] PSBT workflow (create, sign, broadcast)
- [ ] Block pruning
- [ ] Configuration hot-reload
- [ ] Additional JSON-RPC methods (getblock, getrawtransaction, sendrawtransaction, ...)
- [ ] RPC authentication
- [ ] Peer scoring and eviction logic
- [ ] Grafana dashboard templates

---

## Credits

BitcoinWolfe builds on the work of several excellent projects:

- **[libbitcoinkernel](https://github.com/bitcoin/bitcoin/tree/master/src/kernel)** -- Bitcoin Core's consensus engine, extracted as a standalone C library. This is what makes byte-for-byte validation parity possible.
- **[rust-bitcoin](https://github.com/rust-bitcoin/rust-bitcoin)** (0.32) -- Bitcoin data structures, script types, and protocol serialization.
- **[BDK](https://bitcoindevkit.org/)** (1.1) -- Descriptor-based wallet with coin selection, PSBT, and SQLite persistence.
- **[redb](https://github.com/cberner/redb)** -- Pure Rust, ACID-compliant, zero-copy embedded database. No C++ dependencies.
- **[Tokio](https://tokio.rs/)** -- Async runtime powering the P2P and RPC layers.
- **[axum](https://github.com/tokio-rs/axum)** -- HTTP framework for the REST and JSON-RPC server.

---

## License

MIT -- see [LICENSE](LICENSE).
