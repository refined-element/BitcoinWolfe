import { describe, it, expect, vi, beforeEach } from 'vitest';
import { overviewStore } from '$lib/stores/overview.svelte';
import { setConnection } from '$lib/stores/connection.svelte';

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
	return {
		ok: true,
		json: () => Promise.resolve(data),
	} as Response;
}

describe('overview store', () => {
	beforeEach(() => {
		setConnection({ url: 'http://localhost:8332', user: '', password: '', pollInterval: 3000 });
	});

	it('fetches wallet balance and lightning capacity on refresh', async () => {
		const walletInfo = { balance: 2.5, unconfirmed_balance: 0, immature_balance: 0, txcount: 5 };
		const lnChannels = {
			channels: [
				{ channel_id: 'ch1', counterparty: 'abc', capacity_sat: 100000, outbound_capacity_msat: 50000000, inbound_capacity_msat: 50000000, is_usable: true, is_outbound: true, is_channel_ready: true },
				{ channel_id: 'ch2', counterparty: 'def', capacity_sat: 200000, outbound_capacity_msat: 100000000, inbound_capacity_msat: 100000000, is_usable: true, is_outbound: false, is_channel_ready: true },
			],
		};

		vi.mocked(fetch)
			.mockResolvedValueOnce(mockRpcResponse(walletInfo))
			.mockResolvedValueOnce(mockRestResponse(lnChannels));

		const overview = overviewStore();
		await overview.refresh();

		expect(overview.walletAvailable).toBe(true);
		expect(overview.walletBalance).toBe(2.5);
		expect(overview.lnAvailable).toBe(true);
		expect(overview.lnTotalCapacity).toBe(300000);
	});

	it('handles wallet unavailable and lightning unavailable gracefully', async () => {
		vi.mocked(fetch)
			.mockResolvedValueOnce(mockRpcError(-4, 'Wallet not loaded'))
			.mockRejectedValueOnce(new Error('Lightning not enabled'));

		const overview = overviewStore();
		await overview.refresh();

		expect(overview.walletAvailable).toBe(false);
		expect(overview.walletBalance).toBeNull();
		expect(overview.lnAvailable).toBe(false);
		expect(overview.lnTotalCapacity).toBeNull();
	});
});
