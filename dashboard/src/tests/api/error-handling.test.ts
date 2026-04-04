import { describe, it, expect, vi, beforeEach } from 'vitest';
import { fetchRest, rpcCall, ApiError } from '$lib/api/client';
import { setConnection } from '$lib/stores/connection.svelte';

describe('API Error Handling', () => {
	beforeEach(() => {
		setConnection({ url: 'http://localhost:8332', user: '', password: '', pollInterval: 3000 });
	});

	describe('network errors', () => {
		it('throws on network timeout (TypeError)', async () => {
			vi.mocked(fetch).mockRejectedValueOnce(new TypeError('Failed to fetch'));

			await expect(fetchRest('/api/info')).rejects.toThrow(TypeError);
			await expect(fetchRest('/api/info')).rejects.toThrow(); // needs fresh mock
		});

		it('throws on network timeout for rpcCall', async () => {
			vi.mocked(fetch).mockRejectedValueOnce(new TypeError('Failed to fetch'));

			await expect(rpcCall('getblockcount')).rejects.toThrow(TypeError);
		});
	});

	describe('HTTP error responses', () => {
		it('throws ApiError on 401 Unauthorized', async () => {
			vi.mocked(fetch).mockResolvedValueOnce({
				ok: false,
				status: 401,
				statusText: 'Unauthorized',
			} as Response);

			try {
				await fetchRest('/api/info');
				expect.unreachable();
			} catch (e) {
				expect(e).toBeInstanceOf(ApiError);
				expect((e as ApiError).status).toBe(401);
				expect((e as ApiError).message).toBe('401 Unauthorized');
			}
		});

		it('throws ApiError on 403 Forbidden', async () => {
			vi.mocked(fetch).mockResolvedValueOnce({
				ok: false,
				status: 403,
				statusText: 'Forbidden',
			} as Response);

			try {
				await fetchRest('/api/info');
				expect.unreachable();
			} catch (e) {
				expect(e).toBeInstanceOf(ApiError);
				expect((e as ApiError).status).toBe(403);
				expect((e as ApiError).message).toBe('403 Forbidden');
			}
		});

		it('throws ApiError on 500 Internal Server Error', async () => {
			vi.mocked(fetch).mockResolvedValueOnce({
				ok: false,
				status: 500,
				statusText: 'Internal Server Error',
			} as Response);

			try {
				await rpcCall('getblockcount');
				expect.unreachable();
			} catch (e) {
				expect(e).toBeInstanceOf(ApiError);
				expect((e as ApiError).status).toBe(500);
				expect((e as ApiError).message).toBe('500 Internal Server Error');
			}
		});
	});

	describe('malformed responses', () => {
		it('throws when json() fails on REST response', async () => {
			vi.mocked(fetch).mockResolvedValueOnce({
				ok: true,
				json: () => Promise.reject(new SyntaxError('Unexpected token')),
			} as Response);

			await expect(fetchRest('/api/info')).rejects.toThrow(SyntaxError);
		});

		it('throws when json() fails on RPC response', async () => {
			vi.mocked(fetch).mockResolvedValueOnce({
				ok: true,
				json: () => Promise.reject(new SyntaxError('Unexpected end of JSON input')),
			} as Response);

			await expect(rpcCall('getblockcount')).rejects.toThrow(SyntaxError);
		});

		it('handles empty response body (json returns null)', async () => {
			vi.mocked(fetch).mockResolvedValueOnce({
				ok: true,
				json: () => Promise.resolve(null),
			} as Response);

			// fetchRest returns whatever json() returns
			const result = await fetchRest('/api/info');
			expect(result).toBeNull();
		});
	});

	describe('RPC error codes', () => {
		function mockRpcError(code: number, message: string) {
			return {
				ok: true,
				json: () => Promise.resolve({ jsonrpc: '2.0', id: 1, error: { code, message } }),
			} as Response;
		}

		it('throws ApiError with code -1 (miscellaneous)', async () => {
			vi.mocked(fetch).mockResolvedValueOnce(mockRpcError(-1, 'Miscellaneous error'));

			try {
				await rpcCall('badmethod');
				expect.unreachable();
			} catch (e) {
				expect(e).toBeInstanceOf(ApiError);
				expect((e as ApiError).code).toBe(-1);
				expect((e as ApiError).message).toBe('Miscellaneous error');
			}
		});

		it('throws ApiError with code -4 (wallet error)', async () => {
			vi.mocked(fetch).mockResolvedValueOnce(mockRpcError(-4, 'Wallet not loaded'));

			try {
				await rpcCall('getwalletinfo');
				expect.unreachable();
			} catch (e) {
				expect(e).toBeInstanceOf(ApiError);
				expect((e as ApiError).code).toBe(-4);
				expect((e as ApiError).message).toBe('Wallet not loaded');
			}
		});

		it('throws ApiError with code -5 (invalid address)', async () => {
			vi.mocked(fetch).mockResolvedValueOnce(mockRpcError(-5, 'Invalid address'));

			try {
				await rpcCall('validateaddress', ['bad']);
				expect.unreachable();
			} catch (e) {
				expect(e).toBeInstanceOf(ApiError);
				expect((e as ApiError).code).toBe(-5);
				expect((e as ApiError).message).toBe('Invalid address');
			}
		});

		it('throws ApiError with code -6 (insufficient funds)', async () => {
			vi.mocked(fetch).mockResolvedValueOnce(mockRpcError(-6, 'Insufficient funds'));

			try {
				await rpcCall('sendtoaddress', ['addr', 100]);
				expect.unreachable();
			} catch (e) {
				expect(e).toBeInstanceOf(ApiError);
				expect((e as ApiError).code).toBe(-6);
				expect((e as ApiError).message).toBe('Insufficient funds');
			}
		});

		it('throws ApiError with code -32601 (method not found)', async () => {
			vi.mocked(fetch).mockResolvedValueOnce(mockRpcError(-32601, 'Method not found'));

			try {
				await rpcCall('nonexistent');
				expect.unreachable();
			} catch (e) {
				expect(e).toBeInstanceOf(ApiError);
				expect((e as ApiError).code).toBe(-32601);
				expect((e as ApiError).message).toBe('Method not found');
			}
		});

		it('throws ApiError with code -32602 (invalid params)', async () => {
			vi.mocked(fetch).mockResolvedValueOnce(mockRpcError(-32602, 'Invalid params'));

			try {
				await rpcCall('getblock', []);
				expect.unreachable();
			} catch (e) {
				expect(e).toBeInstanceOf(ApiError);
				expect((e as ApiError).code).toBe(-32602);
				expect((e as ApiError).message).toBe('Invalid params');
			}
		});

		it('throws ApiError with code -32603 (internal error)', async () => {
			vi.mocked(fetch).mockResolvedValueOnce(mockRpcError(-32603, 'Internal error'));

			try {
				await rpcCall('getblockcount');
				expect.unreachable();
			} catch (e) {
				expect(e).toBeInstanceOf(ApiError);
				expect((e as ApiError).code).toBe(-32603);
				expect((e as ApiError).message).toBe('Internal error');
			}
		});

		it('ApiError status equals code for RPC errors', async () => {
			vi.mocked(fetch).mockResolvedValueOnce(mockRpcError(-4, 'Wallet not loaded'));

			try {
				await rpcCall('getwalletinfo');
				expect.unreachable();
			} catch (e) {
				const err = e as ApiError;
				expect(err.status).toBe(-4);
				expect(err.code).toBe(-4);
				expect(err.name).toBe('ApiError');
			}
		});
	});
});
