import { describe, it, expect, vi, beforeEach } from 'vitest';
import { walletStore } from '$lib/stores/wallet.svelte';
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

describe('wallet store', () => {
	beforeEach(() => {
		setConnection({ url: 'http://localhost:8332', user: '', password: '', pollInterval: 3000 });
	});

	it('starts in loading state', () => {
		const wallet = walletStore();
		expect(wallet.state).toBe('loading');
		expect(wallet.info).toBeNull();
	});

	it('detects no wallet (error code -4)', async () => {
		vi.mocked(fetch).mockResolvedValueOnce(mockRpcError(-4, 'Wallet not loaded'));

		const wallet = walletStore();
		await wallet.refresh();

		expect(wallet.state).toBe('no_wallet');
		expect(wallet.error).toBeNull();
	});

	it('loads wallet info on success', async () => {
		const walletInfo = { balance: 1.5, unconfirmed_balance: 0.01, immature_balance: 0, txcount: 10 };
		const txList = [{ txid: 'abc', confirmed: true }];

		vi.mocked(fetch)
			.mockResolvedValueOnce(mockRpcResponse(walletInfo))
			.mockResolvedValueOnce(mockRpcResponse(txList));

		const wallet = walletStore();
		await wallet.refresh();

		expect(wallet.state).toBe('loaded');
		expect(wallet.info?.balance).toBe(1.5);
		expect(wallet.transactions).toHaveLength(1);
		expect(wallet.transactions[0].txid).toBe('abc');
	});

	it('sets error state on other errors', async () => {
		vi.mocked(fetch).mockResolvedValueOnce(mockRpcError(-32603, 'Internal error'));

		const wallet = walletStore();
		await wallet.refresh();

		expect(wallet.state).toBe('error');
		expect(wallet.error).toBe('Internal error');
	});
});
