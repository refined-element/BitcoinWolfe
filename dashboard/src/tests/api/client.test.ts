import { describe, it, expect, vi, beforeEach } from 'vitest';
import { fetchRest, rpcCall, ApiError } from '$lib/api/client';
import { setConnection } from '$lib/stores/connection.svelte';

describe('fetchRest', () => {
	beforeEach(() => {
		setConnection({ url: 'http://localhost:8332', user: '', password: '', pollInterval: 3000 });
	});

	it('fetches with correct URL', async () => {
		vi.mocked(fetch).mockResolvedValueOnce({
			ok: true,
			json: () => Promise.resolve({ blocks: 100 }),
		} as Response);

		const result = await fetchRest('/api/info');
		expect(fetch).toHaveBeenCalledWith(
			'http://localhost:8332/api/info',
			expect.objectContaining({ headers: expect.any(Object) })
		);
		expect(result).toEqual({ blocks: 100 });
	});

	it('includes basic auth when credentials set', async () => {
		setConnection({ url: 'http://localhost:8332', user: 'bitcoin', password: 'secret', pollInterval: 3000 });

		vi.mocked(fetch).mockResolvedValueOnce({
			ok: true,
			json: () => Promise.resolve({}),
		} as Response);

		await fetchRest('/api/info');
		const call = vi.mocked(fetch).mock.calls[0];
		const headers = call[1]?.headers as Record<string, string>;
		expect(headers['Authorization']).toBe('Basic ' + btoa('bitcoin:secret'));
	});

	it('omits auth header when no credentials', async () => {
		vi.mocked(fetch).mockResolvedValueOnce({
			ok: true,
			json: () => Promise.resolve({}),
		} as Response);

		await fetchRest('/api/info');
		const call = vi.mocked(fetch).mock.calls[0];
		const headers = call[1]?.headers as Record<string, string>;
		expect(headers['Authorization']).toBeUndefined();
	});

	it('throws ApiError on non-OK response', async () => {
		vi.mocked(fetch).mockResolvedValueOnce({
			ok: false,
			status: 401,
			statusText: 'Unauthorized',
		} as Response);

		await expect(fetchRest('/api/info')).rejects.toThrow(ApiError);
		await expect(fetchRest('/api/info')).rejects.toThrow(); // fetch re-mocked, need separate mock
	});
});

describe('rpcCall', () => {
	beforeEach(() => {
		setConnection({ url: 'http://localhost:8332', user: '', password: '', pollInterval: 3000 });
	});

	it('sends correct JSON-RPC request', async () => {
		vi.mocked(fetch).mockResolvedValueOnce({
			ok: true,
			json: () => Promise.resolve({ jsonrpc: '2.0', id: 1, result: 42 }),
		} as Response);

		const result = await rpcCall('getblockcount');
		expect(result).toBe(42);

		const call = vi.mocked(fetch).mock.calls[0];
		const body = JSON.parse(call[1]?.body as string);
		expect(body.method).toBe('getblockcount');
		expect(body.jsonrpc).toBe('2.0');
		expect(body.params).toEqual([]);
	});

	it('sends params correctly', async () => {
		vi.mocked(fetch).mockResolvedValueOnce({
			ok: true,
			json: () => Promise.resolve({ jsonrpc: '2.0', id: 1, result: 'abc' }),
		} as Response);

		await rpcCall('getblock', ['hash123', 1]);

		const call = vi.mocked(fetch).mock.calls[0];
		const body = JSON.parse(call[1]?.body as string);
		expect(body.params).toEqual(['hash123', 1]);
	});

	it('throws ApiError on RPC error response', async () => {
		vi.mocked(fetch).mockResolvedValueOnce({
			ok: true,
			json: () => Promise.resolve({
				jsonrpc: '2.0',
				id: 1,
				error: { code: -4, message: 'Wallet not loaded' }
			}),
		} as Response);

		try {
			await rpcCall('getwalletinfo');
			expect.unreachable();
		} catch (e) {
			expect(e).toBeInstanceOf(ApiError);
			expect((e as ApiError).code).toBe(-4);
			expect((e as ApiError).message).toBe('Wallet not loaded');
		}
	});

	it('throws ApiError on HTTP error', async () => {
		vi.mocked(fetch).mockResolvedValueOnce({
			ok: false,
			status: 500,
			statusText: 'Internal Server Error',
		} as Response);

		await expect(rpcCall('getblockcount')).rejects.toThrow(ApiError);
	});

	it('posts to root path', async () => {
		vi.mocked(fetch).mockResolvedValueOnce({
			ok: true,
			json: () => Promise.resolve({ jsonrpc: '2.0', id: 1, result: null }),
		} as Response);

		await rpcCall('stop');
		const call = vi.mocked(fetch).mock.calls[0];
		expect(call[0]).toBe('http://localhost:8332/');
		expect((call[1]?.headers as Record<string, string>)['Content-Type']).toBe('application/json');
	});
});
