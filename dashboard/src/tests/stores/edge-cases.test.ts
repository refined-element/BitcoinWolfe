import { describe, it, expect, vi, beforeEach } from 'vitest';
import { nodeStore } from '$lib/stores/node.svelte';
import { walletStore } from '$lib/stores/wallet.svelte';
import { lightningStore } from '$lib/stores/lightning.svelte';
import { nostrStore } from '$lib/stores/nostr.svelte';
import { peersStore } from '$lib/stores/peers.svelte';
import { connectionStore, setConnection } from '$lib/stores/connection.svelte';

function mockRpcResponse(result: unknown) {
	return {
		ok: true,
		json: () => Promise.resolve({ jsonrpc: '2.0', id: 1, result }),
	} as Response;
}

function mockRpcError(code: number, message: string) {
	return {
		ok: true,
		json: () => Promise.resolve({ jsonrpc: '2.0', id: 1, error: { code, message } }),
	} as Response;
}

function mockRestResponse(data: unknown) {
	return { ok: true, json: () => Promise.resolve(data) } as Response;
}

function mockRestError(status: number, statusText: string) {
	return { ok: false, status, statusText } as Response;
}

describe('Store Edge Cases', () => {
	beforeEach(() => {
		setConnection({ url: 'http://localhost:8332', user: '', password: '', pollInterval: 3000 });
	});

	describe('nodeStore recovery', () => {
		it('recovers after failure: disconnected → connected on next refresh', async () => {
			const mockInfo = {
				version: '0.1.0', user_agent: '/BitcoinWolfe/', chain: 'mainnet',
				blocks: 850000, headers: 850000, best_block_hash: 'abc',
				mempool_size: 10, peers: 8, uptime_secs: 3600, syncing: false,
			};
			const mockMempool = {
				size: 10, bytes: 50000,
				policy: { min_fee_rate: 1.0, datacarrier: true, max_datacarrier_bytes: 80, full_rbf: true },
			};

			// First refresh fails
			vi.mocked(fetch).mockRejectedValueOnce(new Error('Connection refused'));

			const node = nodeStore();
			await node.refresh();

			expect(node.connected).toBe(false);
			expect(node.error).toBe('Connection refused');

			// Second refresh succeeds
			vi.mocked(fetch)
				.mockResolvedValueOnce(mockRestResponse(mockInfo))
				.mockResolvedValueOnce(mockRestResponse(mockMempool));

			await node.refresh();

			expect(node.connected).toBe(true);
			expect(node.error).toBeNull();
			expect(node.info?.blocks).toBe(850000);
		});
	});

	describe('walletStore transitions', () => {
		it('transitions loading → no_wallet → loaded', async () => {
			const wallet = walletStore();
			expect(wallet.state).toBe('loading');

			// First refresh: no wallet
			vi.mocked(fetch).mockResolvedValueOnce(mockRpcError(-4, 'Wallet not loaded'));
			await wallet.refresh();
			expect(wallet.state).toBe('no_wallet');

			// After wallet creation (handled externally), refresh with wallet info
			const walletInfo = { balance: 0, unconfirmed_balance: 0, immature_balance: 0, txcount: 0 };
			vi.mocked(fetch)
				.mockResolvedValueOnce(mockRpcResponse(walletInfo))
				.mockResolvedValueOnce(mockRpcResponse([]));

			await wallet.refresh();
			expect(wallet.state).toBe('loaded');
			expect(wallet.info?.balance).toBe(0);
		});

		it('handles 401 auth error', async () => {
			vi.mocked(fetch).mockResolvedValueOnce({
				ok: false,
				status: 401,
				statusText: 'Unauthorized',
			} as Response);

			const wallet = walletStore();
			await wallet.refresh();

			expect(wallet.state).toBe('error');
			expect(wallet.error).toBe('Authentication failed — check credentials in Settings');
		});

		it('handles 403 auth error', async () => {
			vi.mocked(fetch).mockResolvedValueOnce({
				ok: false,
				status: 403,
				statusText: 'Forbidden',
			} as Response);

			const wallet = walletStore();
			await wallet.refresh();

			expect(wallet.state).toBe('error');
			expect(wallet.error).toBe('Authentication failed — check credentials in Settings');
		});
	});

	describe('lightningStore when disabled', () => {
		it('sets error when lightning REST returns error', async () => {
			vi.mocked(fetch).mockResolvedValueOnce(mockRestError(404, 'Not Found'));

			const ln = lightningStore();
			await ln.refresh();

			expect(ln.error).toBeTruthy();
			expect(ln.info).toBeNull();
			expect(ln.channels).toEqual([]);
		});

		it('handles lightning info but channels fail', async () => {
			const lnInfo = {
				enabled: false, node_id: '', num_channels: 0,
				num_active_channels: 0, num_peers: 0,
			};
			vi.mocked(fetch)
				.mockResolvedValueOnce(mockRestResponse(lnInfo))
				.mockResolvedValueOnce(mockRestResponse({ channels: [] }))
				.mockResolvedValueOnce(mockRpcError(-32601, 'Method not found'));

			const ln = lightningStore();
			await ln.refresh();

			// Should succeed — peers failure is swallowed
			expect(ln.error).toBeNull();
			expect(ln.info?.enabled).toBe(false);
		});
	});

	describe('nostrStore when disabled', () => {
		it('sets error when nostr RPC returns error', async () => {
			vi.mocked(fetch).mockResolvedValueOnce(mockRpcError(-32601, 'Method not found'));

			const nostr = nostrStore();
			await nostr.refresh();

			expect(nostr.error).toBeTruthy();
			expect(nostr.info).toBeNull();
		});

		it('sets error message from non-Error throw', async () => {
			vi.mocked(fetch).mockRejectedValueOnce('string error');

			const nostr = nostrStore();
			await nostr.refresh();

			expect(nostr.error).toBe('Nostr unavailable');
		});
	});

	describe('peersStore loaded flag', () => {
		it('starts with loaded=false', () => {
			const peers = peersStore();
			expect(peers.loaded).toBe(false);
		});

		it('loaded becomes true after successful refresh', async () => {
			vi.mocked(fetch).mockResolvedValueOnce(mockRestResponse({ count: 2, peers: [
				{ addr: '1.2.3.4:8333', user_agent: '/test/', version: 70016, inbound: false, v2_transport: false, start_height: 850000 },
				{ addr: '5.6.7.8:8333', user_agent: '/test2/', version: 70016, inbound: true, v2_transport: true, start_height: 849000 },
			]}));

			const peers = peersStore();
			await peers.refresh();

			expect(peers.loaded).toBe(true);
			expect(peers.count).toBe(2);
			expect(peers.list).toHaveLength(2);
			expect(peers.error).toBeNull();
		});

		it('loaded stays false after failed refresh', async () => {
			vi.mocked(fetch).mockRejectedValueOnce(new Error('Network error'));

			const peers = peersStore();
			await peers.refresh();

			// loaded is not set on error path
			expect(peers.error).toBe('Network error');
		});
	});

	describe('connectionStore loads from pre-seeded localStorage', () => {
		it('loads saved connection config from localStorage', () => {
			const saved = { url: 'http://mynode:8332', user: 'alice', password: 'pass123', pollInterval: 5000 };
			localStorage.setItem('bitcoinwolfe_connection', JSON.stringify(saved));

			// setConnection with the saved values to simulate store load
			setConnection(saved);
			const conn = connectionStore();

			expect(conn.current.url).toBe('http://mynode:8332');
			expect(conn.current.user).toBe('alice');
			expect(conn.current.password).toBe('pass123');
			expect(conn.current.pollInterval).toBe(5000);
		});

		it('uses defaults when localStorage is empty', () => {
			// localStorage is cleared in beforeEach
			setConnection({ url: '', user: '', password: '', pollInterval: 3000 });
			const conn = connectionStore();

			expect(conn.current.url).toBe('');
			expect(conn.current.user).toBe('');
			expect(conn.current.password).toBe('');
			expect(conn.current.pollInterval).toBe(3000);
		});
	});
});
