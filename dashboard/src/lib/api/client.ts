import { getConnection } from '../stores/connection.svelte';

export class ApiError extends Error {
	constructor(
		public status: number,
		message: string,
		public code?: number
	) {
		super(message);
		this.name = 'ApiError';
	}
}

function getHeaders(): HeadersInit {
	const conn = getConnection();
	const headers: HeadersInit = { 'Content-Type': 'application/json' };
	if (conn.user && conn.password) {
		headers['Authorization'] = 'Basic ' + btoa(`${conn.user}:${conn.password}`);
	}
	return headers;
}

function getBaseUrl(): string {
	const conn = getConnection();
	return conn.url || '';
}

export async function fetchRest<T>(path: string): Promise<T> {
	const res = await fetch(`${getBaseUrl()}${path}`, { headers: getHeaders() });
	if (!res.ok) {
		throw new ApiError(res.status, `${res.status} ${res.statusText}`);
	}
	return res.json();
}

export async function rpcCall<T>(method: string, params?: unknown[]): Promise<T> {
	const res = await fetch(`${getBaseUrl()}/`, {
		method: 'POST',
		headers: getHeaders(),
		body: JSON.stringify({
			jsonrpc: '2.0',
			id: Date.now(),
			method,
			params: params ?? []
		})
	});

	if (!res.ok) {
		throw new ApiError(res.status, `${res.status} ${res.statusText}`);
	}

	const data = await res.json();
	if (data.error) {
		throw new ApiError(data.error.code, data.error.message, data.error.code);
	}
	return data.result as T;
}
