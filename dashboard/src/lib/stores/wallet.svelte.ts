import type { WalletInfo, WalletTransaction } from '../api/types';
import { getWalletInfo, listTransactions } from '../api/rpc';

export type WalletState = 'loading' | 'no_wallet' | 'loaded' | 'error';

let walletInfo = $state<WalletInfo | null>(null);
let transactions = $state<WalletTransaction[]>([]);
let walletState = $state<WalletState>('loading');
let walletError = $state<string | null>(null);

export function walletStore() {
	return {
		get info() { return walletInfo; },
		get transactions() { return transactions; },
		get state() { return walletState; },
		get error() { return walletError; },

		async refresh() {
			try {
				const info = await getWalletInfo();
				walletInfo = info;
				const txs = await listTransactions();
				transactions = txs;
				walletState = 'loaded';
				walletError = null;
			} catch (e: unknown) {
				const err = e as { code?: number; status?: number; message?: string };
				if (err.code === -4 || err.code === -18) {
					// -4 = wallet error, -18 = wallet not found
					walletState = 'no_wallet';
					walletError = null;
				} else if (err.status === 401 || err.status === 403) {
					walletState = 'error';
					walletError = 'Authentication failed — check credentials in Settings';
				} else {
					walletState = 'error';
					walletError = err.message ?? 'Unknown error';
				}
			}
		}
	};
}
