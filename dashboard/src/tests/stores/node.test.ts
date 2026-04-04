import { describe, it, expect, vi, beforeEach } from 'vitest';
import { nodeStore } from '$lib/stores/node.svelte';
import { setConnection } from '$lib/stores/connection.svelte';

describe('node store', () => {
	beforeEach(() => {
		setConnection({ url: 'http://localhost:8332', user: '', password: '', pollInterval: 3000 });
	});

	it('starts disconnected with null info', () => {
		const node = nodeStore();
		expect(node.info).toBeNull();
		expect(node.mempool).toBeNull();
		expect(node.connected).toBe(false);
		expect(node.error).toBeNull();
	});

	it('populates on successful refresh', async () => {
		const mockInfo = {
			version: '0.1.0', user_agent: '/BitcoinWolfe:0.1.0/', chain: 'mainnet',
			blocks: 850000, headers: 850000, best_block_hash: 'abc123',
			mempool_size: 42, peers: 8, uptime_secs: 3600, syncing: false,
		};
		const mockMempool = {
			size: 42, bytes: 100000,
			policy: { min_fee_rate: 1.0, datacarrier: true, max_datacarrier_bytes: 80, full_rbf: true },
		};

		vi.mocked(fetch)
			.mockResolvedValueOnce({ ok: true, json: () => Promise.resolve(mockInfo) } as Response)
			.mockResolvedValueOnce({ ok: true, json: () => Promise.resolve(mockMempool) } as Response);

		const node = nodeStore();
		await node.refresh();

		expect(node.connected).toBe(true);
		expect(node.info?.blocks).toBe(850000);
		expect(node.info?.chain).toBe('mainnet');
		expect(node.mempool?.size).toBe(42);
		expect(node.error).toBeNull();
	});

	it('sets error on failed refresh', async () => {
		vi.mocked(fetch).mockRejectedValueOnce(new Error('Connection refused'));

		const node = nodeStore();
		await node.refresh();

		expect(node.connected).toBe(false);
		expect(node.error).toBe('Connection refused');
	});
});
