import { describe, it, expect, vi, beforeEach } from 'vitest';
import { lightningStore } from '$lib/stores/lightning.svelte';
import { setConnection } from '$lib/stores/connection.svelte';
import { rpcCall } from '$lib/api/client';

function mockRpcResponse(result: unknown) {
	return {
		ok: true,
		json: () => Promise.resolve({ jsonrpc: '2.0', id: 1, result }),
	} as Response;
}

function mockRestResponse(data: unknown) {
	return { ok: true, json: () => Promise.resolve(data) } as Response;
}

function mockRestError(status: number, statusText: string) {
	return { ok: false, status, statusText } as Response;
}

describe('Lightning Channel Flow', () => {
	beforeEach(() => {
		setConnection({ url: 'http://localhost:8332', user: 'bitcoin', password: 'secret', pollInterval: 3000 });
	});

	it('simulates: no channels → connect peer → open channel → refresh shows channel', async () => {
		const ln = lightningStore();

		// Step 1: Initial refresh — no channels
		const lnInfo = {
			enabled: true, node_id: '03abc...', num_channels: 0,
			num_active_channels: 0, num_peers: 0,
		};
		vi.mocked(fetch)
			.mockResolvedValueOnce(mockRestResponse(lnInfo))
			.mockResolvedValueOnce(mockRestResponse({ channels: [] }))
			.mockResolvedValueOnce(mockRpcResponse([])); // ln_listpeers

		await ln.refresh();
		expect(ln.info?.enabled).toBe(true);
		expect(ln.channels).toEqual([]);
		expect(ln.peers).toEqual([]);
		expect(ln.error).toBeNull();

		// Step 2: Connect to a peer (via rpcCall)
		vi.mocked(fetch).mockResolvedValueOnce(
			mockRpcResponse({ connected: true })
		);
		const connectResult = await rpcCall<{ connected: boolean }>('ln_connect', ['03def456@1.2.3.4:9735']);
		expect(connectResult.connected).toBe(true);

		// Step 3: Open a channel (via rpcCall)
		vi.mocked(fetch).mockResolvedValueOnce(
			mockRpcResponse({ channel_id: 'chan_abc123' })
		);
		const openResult = await rpcCall<{ channel_id: string }>('ln_openchannel', ['03def456', 100000, 0]);
		expect(openResult.channel_id).toBe('chan_abc123');

		// Step 4: Refresh — now shows channel and peer
		const updatedInfo = {
			enabled: true, node_id: '03abc...', num_channels: 1,
			num_active_channels: 1, num_peers: 1,
		};
		const channel = {
			channel_id: 'chan_abc123',
			counterparty: '03def456',
			capacity_sat: 100000,
			outbound_capacity_msat: 99000000,
			inbound_capacity_msat: 0,
			is_usable: true,
			is_outbound: true,
			is_channel_ready: true,
			short_channel_id: '850000x1x0',
		};
		const peer = { node_id: '03def456', address: '1.2.3.4:9735', inbound: false };

		vi.mocked(fetch)
			.mockResolvedValueOnce(mockRestResponse(updatedInfo))
			.mockResolvedValueOnce(mockRestResponse({ channels: [channel] }))
			.mockResolvedValueOnce(mockRpcResponse([peer]));

		await ln.refresh();
		expect(ln.info?.num_channels).toBe(1);
		expect(ln.channels).toHaveLength(1);
		expect(ln.channels[0].channel_id).toBe('chan_abc123');
		expect(ln.channels[0].capacity_sat).toBe(100000);
		expect(ln.peers).toHaveLength(1);
		expect(ln.peers[0].node_id).toBe('03def456');
	});

	it('simulates cooperative channel close flow', async () => {
		const ln = lightningStore();

		// Initial state: 1 channel
		vi.mocked(fetch)
			.mockResolvedValueOnce(mockRestResponse({
				enabled: true, node_id: '03abc', num_channels: 1,
				num_active_channels: 1, num_peers: 1,
			}))
			.mockResolvedValueOnce(mockRestResponse({ channels: [{
				channel_id: 'chan_123', counterparty: '03def', capacity_sat: 50000,
				outbound_capacity_msat: 25000000, inbound_capacity_msat: 25000000,
				is_usable: true, is_outbound: true, is_channel_ready: true,
			}]}))
			.mockResolvedValueOnce(mockRpcResponse([{ node_id: '03def', address: '5.6.7.8:9735', inbound: false }]));

		await ln.refresh();
		expect(ln.channels).toHaveLength(1);

		// Close the channel cooperatively
		vi.mocked(fetch).mockResolvedValueOnce(
			mockRpcResponse({ closing: true, force: false })
		);
		const closeResult = await rpcCall<{ closing: boolean; force: boolean }>('ln_closechannel', ['chan_123', '03def', false]);
		expect(closeResult.closing).toBe(true);
		expect(closeResult.force).toBe(false);

		// Refresh — channel is gone
		vi.mocked(fetch)
			.mockResolvedValueOnce(mockRestResponse({
				enabled: true, node_id: '03abc', num_channels: 0,
				num_active_channels: 0, num_peers: 0,
			}))
			.mockResolvedValueOnce(mockRestResponse({ channels: [] }))
			.mockResolvedValueOnce(mockRpcResponse([]));

		await ln.refresh();
		expect(ln.channels).toHaveLength(0);
		expect(ln.info?.num_channels).toBe(0);
	});

	it('simulates force channel close flow', async () => {
		const ln = lightningStore();

		// Initial state: 1 channel
		vi.mocked(fetch)
			.mockResolvedValueOnce(mockRestResponse({
				enabled: true, node_id: '03abc', num_channels: 1,
				num_active_channels: 0, num_peers: 0,
			}))
			.mockResolvedValueOnce(mockRestResponse({ channels: [{
				channel_id: 'chan_stuck', counterparty: '03ghost', capacity_sat: 200000,
				outbound_capacity_msat: 100000000, inbound_capacity_msat: 100000000,
				is_usable: false, is_outbound: true, is_channel_ready: false,
			}]}))
			.mockResolvedValueOnce(mockRpcResponse([])); // no peers connected

		await ln.refresh();
		expect(ln.channels).toHaveLength(1);
		expect(ln.channels[0].is_usable).toBe(false);

		// Force close
		vi.mocked(fetch).mockResolvedValueOnce(
			mockRpcResponse({ closing: true, force: true })
		);
		const closeResult = await rpcCall<{ closing: boolean; force: boolean }>('ln_closechannel', ['chan_stuck', '03ghost', true]);
		expect(closeResult.closing).toBe(true);
		expect(closeResult.force).toBe(true);

		// Refresh — channel gone
		vi.mocked(fetch)
			.mockResolvedValueOnce(mockRestResponse({
				enabled: true, node_id: '03abc', num_channels: 0,
				num_active_channels: 0, num_peers: 0,
			}))
			.mockResolvedValueOnce(mockRestResponse({ channels: [] }))
			.mockResolvedValueOnce(mockRpcResponse([]));

		await ln.refresh();
		expect(ln.channels).toHaveLength(0);
	});

	it('handles lightning disabled scenario gracefully', async () => {
		const ln = lightningStore();

		// REST endpoint returns 404 when lightning is disabled
		vi.mocked(fetch).mockResolvedValueOnce(mockRestError(404, 'Not Found'));

		await ln.refresh();
		expect(ln.error).toBeTruthy();
		// Note: ln.info may retain value from previous test due to module-level state,
		// but channels are reset to [] and error is set
		expect(ln.channels).toEqual([]);
	});
});
