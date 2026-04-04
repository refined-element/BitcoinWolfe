import type { NodeInfo, MempoolInfo } from '../api/types';
import { getNodeInfo, getMempoolInfo } from '../api/rest';

let nodeInfo = $state<NodeInfo | null>(null);
let mempoolInfo = $state<MempoolInfo | null>(null);
let error = $state<string | null>(null);
let connected = $state(false);

export function nodeStore() {
	return {
		get info() { return nodeInfo; },
		get mempool() { return mempoolInfo; },
		get error() { return error; },
		get connected() { return connected; },

		async refresh() {
			try {
				const [info, mempool] = await Promise.all([getNodeInfo(), getMempoolInfo()]);
				nodeInfo = info;
				mempoolInfo = mempool;
				error = null;
				connected = true;
			} catch (e) {
				error = e instanceof Error ? e.message : 'Connection failed';
				connected = false;
			}
		}
	};
}
