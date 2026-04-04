// ── Node Info ──────────────────────────────────────────────
export interface NodeInfo {
  version: string;
  user_agent: string;
  chain: string;
  blocks: number;
  headers: number;
  best_block_hash: string;
  mempool_size: number;
  peers: number;
  uptime_secs: number;
  syncing: boolean;
}

// ── Mempool ────────────────────────────────────────────────
export interface MempoolInfo {
  size: number;
  bytes: number;
  policy: {
    min_fee_rate: number;
    datacarrier: boolean;
    max_datacarrier_bytes: number;
    full_rbf: boolean;
  };
}

// ── Peers ──────────────────────────────────────────────────
export interface PeerInfo {
  addr: string;
  user_agent: string;
  version: number;
  inbound: boolean;
  v2_transport: boolean;
  start_height: number;
}

export interface PeersResponse {
  count: number;
  peers: PeerInfo[];
}

// ── Wallet ─────────────────────────────────────────────────
export interface WalletInfo {
  balance: number;
  unconfirmed_balance: number;
  immature_balance: number;
  txcount: number;
}

export interface WalletTransaction {
  txid: string;
  confirmed: boolean;
}

export interface CreateWalletResult {
  mnemonic: string;
  message: string;
}

export interface PsbtResult {
  psbt: string;
  fee: number;
  changepos: number;
}

export interface ProcessPsbtResult {
  psbt: string;
  complete: boolean;
}

export interface RescanResult {
  start_height: number;
  stop_height: number;
  blocks_scanned: number;
  transactions_found: number;
}

// ── Lightning ──────────────────────────────────────────────
export interface LightningInfo {
  enabled: boolean;
  node_id: string;
  num_channels: number;
  num_active_channels: number;
  num_peers: number;
}

export interface LightningChannel {
  channel_id: string;
  counterparty: string;
  capacity_sat: number;
  outbound_capacity_msat: number;
  inbound_capacity_msat: number;
  is_usable: boolean;
  is_outbound: boolean;
  is_channel_ready: boolean;
  short_channel_id?: string;
}

export interface LightningPeer {
  node_id: string;
  address: string;
  inbound: boolean;
}

export interface LightningPayment {
  payment_hash: string;
  direction: string;
  status: string;
  amount_msat: number | null;
  fee_msat: number | null;
  timestamp: number;
}

// ── Nostr ──────────────────────────────────────────────────
export interface NostrInfo {
  enabled: boolean;
  npub: string;
  relay_count: number;
  relays: string[];
}

export interface NostrRelay {
  url: string;
}

// ── JSON-RPC ───────────────────────────────────────────────
export interface JsonRpcResponse<T = unknown> {
  jsonrpc: string;
  id: number | string;
  result?: T;
  error?: { code: number; message: string };
}

// ── Connection ─────────────────────────────────────────────
export interface ConnectionConfig {
  url: string;
  user: string;
  password: string;
  pollInterval: number;
}
