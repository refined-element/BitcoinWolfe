import type { WalletInfo, LightningChannel } from '../api/types';
import { getWalletInfo } from '../api/rpc';
import { getLightningChannels } from '../api/rest';

let walletBalance = $state<number | null>(null);
let walletAvailable = $state(false);
let lnTotalCapacity = $state<number | null>(null);
let lnAvailable = $state(false);

export function overviewStore() {
	return {
		get walletBalance() { return walletBalance; },
		get walletAvailable() { return walletAvailable; },
		get lnTotalCapacity() { return lnTotalCapacity; },
		get lnAvailable() { return lnAvailable; },

		async refresh() {
			// Fetch wallet balance — silently skip if wallet not available
			try {
				const info = await getWalletInfo();
				walletBalance = info.balance;
				walletAvailable = true;
			} catch {
				walletBalance = null;
				walletAvailable = false;
			}

			// Fetch lightning channels — silently skip if lightning not available
			try {
				const data = await getLightningChannels();
				const total = data.channels.reduce((sum, ch) => sum + ch.capacity_sat, 0);
				lnTotalCapacity = total;
				lnAvailable = true;
			} catch {
				lnTotalCapacity = null;
				lnAvailable = false;
			}
		}
	};
}
