import type { NostrInfo } from '../api/types';
import { nostrGetInfo } from '../api/rpc';

let nostrInfo = $state<NostrInfo | null>(null);
let nostrError = $state<string | null>(null);

export function nostrStore() {
	return {
		get info() { return nostrInfo; },
		get error() { return nostrError; },

		async refresh() {
			try {
				nostrInfo = await nostrGetInfo();
				nostrError = null;
			} catch (e) {
				nostrError = e instanceof Error ? e.message : 'Nostr unavailable';
			}
		}
	};
}
