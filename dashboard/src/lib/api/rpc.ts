import { rpcCall } from './client';
import type {
	WalletInfo, WalletTransaction, CreateWalletResult,
	PsbtResult, ProcessPsbtResult, RescanResult,
	LightningInfo, LightningChannel, LightningPeer,
	NostrInfo, NostrRelay
} from './types';

// ── Wallet ─────────────────────────────────────────────────
export const getWalletInfo = () => rpcCall<WalletInfo>('getwalletinfo');
export const getBalance = () => rpcCall<number>('getbalance');
export const getNewAddress = () => rpcCall<string>('getnewaddress');
export const listTransactions = () => rpcCall<WalletTransaction[]>('listtransactions');
export const createWallet = () => rpcCall<CreateWalletResult>('createwallet');
export const importWallet = (mnemonic: string) => rpcCall<{ message: string }>('importwallet', [mnemonic]);
export const createFundedPsbt = (outputs: Record<string, number>, feeRate?: number) =>
	rpcCall<PsbtResult>('walletcreatefundedpsbt', [[], [outputs], 0, feeRate ? { fee_rate: feeRate } : {}]);
export const processPsbt = (psbt: string) => rpcCall<ProcessPsbtResult>('walletprocesspsbt', [psbt]);
export const sendRawTransaction = (hex: string) => rpcCall<string>('sendrawtransaction', [hex]);
export const rescanBlockchain = (start?: number, stop?: number) =>
	rpcCall<RescanResult>('rescanblockchain', start !== undefined ? [start, stop] : []);

// ── Lightning ──────────────────────────────────────────────
export const lnGetInfo = () => rpcCall<LightningInfo>('ln_getinfo');
export const lnConnect = (target: string) => rpcCall<{ connected: boolean }>('ln_connect', [target]);
export const lnOpenChannel = (nodeId: string, amountSat: number, pushMsat?: number) =>
	rpcCall<{ channel_id: string }>('ln_openchannel', [nodeId, amountSat, pushMsat ?? 0]);
export const lnCloseChannel = (channelId: string, counterparty: string, force?: boolean) =>
	rpcCall<{ closing: boolean; force: boolean }>('ln_closechannel', [channelId, counterparty, force ?? false]);
export const lnCreateInvoice = (amountMsat?: number, description?: string, expiry?: number) =>
	rpcCall<{ invoice: string }>('ln_invoice', [amountMsat, description, expiry]);
export const lnPay = (invoice: string) => rpcCall<{ payment_id: string }>('ln_pay', [invoice]);
export const lnListChannels = () => rpcCall<LightningChannel[]>('ln_listchannels');
export const lnListPeers = () => rpcCall<LightningPeer[]>('ln_listpeers');

// ── Nostr ──────────────────────────────────────────────────
export const nostrGetInfo = () => rpcCall<NostrInfo>('nostr_getinfo');
export const nostrPublish = (content: string, kind?: number) =>
	rpcCall<{ event_id: string; kind: number }>('nostr_publish', kind ? [content, kind] : [content]);
export const nostrAddRelay = (url: string) => rpcCall<{ added: boolean }>('nostr_addrelay', [url]);
export const nostrRemoveRelay = (url: string) => rpcCall<{ removed: boolean }>('nostr_removerelay', [url]);
export const nostrListRelays = () => rpcCall<NostrRelay[]>('nostr_listrelays');

// ── Utility ────────────────────────────────────────────────
export const stopNode = () => rpcCall<string>('stop');
