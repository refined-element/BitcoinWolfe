import type { PeerInfo } from '../api/types';
import { getPeers } from '../api/rest';

let peerList = $state<PeerInfo[]>([]);
let peerCount = $state(0);
let peerError = $state<string | null>(null);
let peerLoaded = $state(false);

export function peersStore() {
	return {
		get list() { return peerList; },
		get count() { return peerCount; },
		get error() { return peerError; },
		get loaded() { return peerLoaded; },

		async refresh() {
			try {
				const data = await getPeers();
				peerList = data.peers;
				peerCount = data.count;
				peerError = null;
				peerLoaded = true;
			} catch (e) {
				peerError = e instanceof Error ? e.message : 'Failed to fetch peers';
			}
		}
	};
}
