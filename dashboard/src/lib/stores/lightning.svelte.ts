import type { LightningInfo, LightningChannel, LightningPeer } from '../api/types';
import { getLightningInfo, getLightningChannels } from '../api/rest';
import { lnListPeers } from '../api/rpc';

let lnInfo = $state<LightningInfo | null>(null);
let channels = $state<LightningChannel[]>([]);
let peers = $state<LightningPeer[]>([]);
let lnError = $state<string | null>(null);

export function lightningStore() {
	return {
		get info() { return lnInfo; },
		get channels() { return channels; },
		get peers() { return peers; },
		get error() { return lnError; },

		async refresh() {
			try {
				const [info, chans] = await Promise.all([
					getLightningInfo(),
					getLightningChannels()
				]);
				lnInfo = info;
				channels = chans.channels;
				try {
					peers = await lnListPeers();
				} catch {
					// peers may fail if LN is not enabled
				}
				lnError = null;
			} catch (e) {
				lnError = e instanceof Error ? e.message : 'Lightning unavailable';
			}
		}
	};
}
