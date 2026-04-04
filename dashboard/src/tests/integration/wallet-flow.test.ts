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

describe('Wallet Creation Flow', () => {
	beforeEach(() => {
		setConnection({ url: 'http://localhost:8332', user: 'bitcoin', password: 'secret', pollInterval: 3000 });
	});

	it('simulates full wallet creation: no wallet → create → refresh → loaded', async () => {
		const wallet = walletStore();

		// Step 1: First refresh — wallet not loaded (code -4)
		vi.mocked(fetch).mockResolvedValueOnce(mockRpcError(-4, 'Wallet not loaded'));
		await wallet.refresh();
		expect(wallet.state).toBe('no_wallet');
		expect(wallet.info).toBeNull();

		// Step 2: createWallet RPC call (simulated via rpcCall)
		// This is just verifying the mock setup — in real app, the component calls createWallet()
		vi.mocked(fetch).mockResolvedValueOnce(
			mockRpcResponse({ mnemonic: 'abandon abandon abandon abandon abandon abandon abandon abandon abandon abandon abandon about', message: 'Wallet created' })
		);
		const createRes = await fetch('http://localhost:8332/', {
			method: 'POST',
			body: JSON.stringify({ jsonrpc: '2.0', id: 1, method: 'createwallet', params: [] }),
		});
		const createData = await createRes.json();
		expect(createData.result.mnemonic).toBeTruthy();

		// Step 3: Refresh after creation — wallet is now loaded
		const walletInfo = { balance: 0, unconfirmed_balance: 0, immature_balance: 0, txcount: 0 };
		vi.mocked(fetch)
			.mockResolvedValueOnce(mockRpcResponse(walletInfo))
			.mockResolvedValueOnce(mockRpcResponse([]));

		await wallet.refresh();
		expect(wallet.state).toBe('loaded');
		expect(wallet.info?.balance).toBe(0);
		expect(wallet.transactions).toEqual([]);
		expect(wallet.error).toBeNull();
	});

	it('simulates wallet import flow: import → refresh → loaded', async () => {
		const wallet = walletStore();

		// Step 1: Wallet not found
		vi.mocked(fetch).mockResolvedValueOnce(mockRpcError(-18, 'Wallet not found'));
		await wallet.refresh();
		expect(wallet.state).toBe('no_wallet');

		// Step 2: Import wallet RPC
		vi.mocked(fetch).mockResolvedValueOnce(
			mockRpcResponse({ message: 'Wallet imported successfully' })
		);
		const importRes = await fetch('http://localhost:8332/', {
			method: 'POST',
			body: JSON.stringify({ jsonrpc: '2.0', id: 1, method: 'importwallet', params: ['word1 word2 word3 word4 word5 word6 word7 word8 word9 word10 word11 word12'] }),
		});
		const importData = await importRes.json();
		expect(importData.result.message).toBe('Wallet imported successfully');

		// Step 3: Refresh — wallet loaded with balance
		const walletInfo = { balance: 2.5, unconfirmed_balance: 0.1, immature_balance: 0, txcount: 5 };
		const txs = [
			{ txid: 'tx1', confirmed: true },
			{ txid: 'tx2', confirmed: true },
			{ txid: 'tx3', confirmed: false },
		];
		vi.mocked(fetch)
			.mockResolvedValueOnce(mockRpcResponse(walletInfo))
			.mockResolvedValueOnce(mockRpcResponse(txs));

		await wallet.refresh();
		expect(wallet.state).toBe('loaded');
		expect(wallet.info?.balance).toBe(2.5);
		expect(wallet.info?.unconfirmed_balance).toBe(0.1);
		expect(wallet.transactions).toHaveLength(3);
	});

	it('wallet balance updates on each refresh cycle', async () => {
		const wallet = walletStore();

		// First refresh: balance = 1.0
		vi.mocked(fetch)
			.mockResolvedValueOnce(mockRpcResponse({ balance: 1.0, unconfirmed_balance: 0, immature_balance: 0, txcount: 2 }))
			.mockResolvedValueOnce(mockRpcResponse([{ txid: 'a', confirmed: true }, { txid: 'b', confirmed: true }]));
		await wallet.refresh();
		expect(wallet.info?.balance).toBe(1.0);
		expect(wallet.transactions).toHaveLength(2);

		// Second refresh: balance increased to 2.5
		vi.mocked(fetch)
			.mockResolvedValueOnce(mockRpcResponse({ balance: 2.5, unconfirmed_balance: 0.5, immature_balance: 0, txcount: 4 }))
			.mockResolvedValueOnce(mockRpcResponse([
				{ txid: 'a', confirmed: true },
				{ txid: 'b', confirmed: true },
				{ txid: 'c', confirmed: true },
				{ txid: 'd', confirmed: false },
			]));
		await wallet.refresh();
		expect(wallet.info?.balance).toBe(2.5);
		expect(wallet.info?.unconfirmed_balance).toBe(0.5);
		expect(wallet.transactions).toHaveLength(4);

		// Third refresh: balance decreased (sent some BTC)
		vi.mocked(fetch)
			.mockResolvedValueOnce(mockRpcResponse({ balance: 1.2, unconfirmed_balance: 0, immature_balance: 0, txcount: 5 }))
			.mockResolvedValueOnce(mockRpcResponse([
				{ txid: 'a', confirmed: true },
				{ txid: 'b', confirmed: true },
				{ txid: 'c', confirmed: true },
				{ txid: 'd', confirmed: true },
				{ txid: 'e', confirmed: true },
			]));
		await wallet.refresh();
		expect(wallet.info?.balance).toBe(1.2);
		expect(wallet.transactions).toHaveLength(5);
	});

	it('handles intermittent errors during refresh cycles', async () => {
		const wallet = walletStore();

		// First refresh succeeds
		vi.mocked(fetch)
			.mockResolvedValueOnce(mockRpcResponse({ balance: 1.0, unconfirmed_balance: 0, immature_balance: 0, txcount: 1 }))
			.mockResolvedValueOnce(mockRpcResponse([{ txid: 'tx1', confirmed: true }]));
		await wallet.refresh();
		expect(wallet.state).toBe('loaded');

		// Second refresh fails with internal error
		vi.mocked(fetch).mockResolvedValueOnce(mockRpcError(-32603, 'Database locked'));
		await wallet.refresh();
		expect(wallet.state).toBe('error');
		expect(wallet.error).toBe('Database locked');

		// Third refresh recovers
		vi.mocked(fetch)
			.mockResolvedValueOnce(mockRpcResponse({ balance: 1.0, unconfirmed_balance: 0, immature_balance: 0, txcount: 1 }))
			.mockResolvedValueOnce(mockRpcResponse([{ txid: 'tx1', confirmed: true }]));
		await wallet.refresh();
		expect(wallet.state).toBe('loaded');
		expect(wallet.error).toBeNull();
	});
});
