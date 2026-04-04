import { describe, it, expect, beforeEach } from 'vitest';
import { getConnection, setConnection, connectionStore } from '$lib/stores/connection.svelte';

describe('connection store', () => {
	beforeEach(() => {
		// Reset to defaults
		setConnection({ url: '', user: '', password: '', pollInterval: 3000 });
	});

	it('returns defaults when no localStorage', () => {
		const conn = getConnection();
		expect(conn.url).toBe('');
		expect(conn.user).toBe('');
		expect(conn.password).toBe('');
		expect(conn.pollInterval).toBe(3000);
	});

	it('updates connection settings', () => {
		setConnection({ url: 'http://mynode:8332', user: 'admin' });
		const conn = getConnection();
		expect(conn.url).toBe('http://mynode:8332');
		expect(conn.user).toBe('admin');
		expect(conn.password).toBe(''); // unchanged
	});

	it('persists to localStorage', () => {
		setConnection({ url: 'http://test:8332' });
		const stored = JSON.parse(localStorage.getItem('bitcoinwolfe_connection')!);
		expect(stored.url).toBe('http://test:8332');
	});

	it('connectionStore provides getter and setter', () => {
		const store = connectionStore();
		store.set({ url: 'http://via-store:8332' });
		expect(store.current.url).toBe('http://via-store:8332');
	});

	it('partial updates preserve other fields', () => {
		setConnection({ url: 'http://a:8332', user: 'bob', password: 'pass', pollInterval: 5000 });
		setConnection({ user: 'alice' });
		const conn = getConnection();
		expect(conn.url).toBe('http://a:8332');
		expect(conn.user).toBe('alice');
		expect(conn.password).toBe('pass');
		expect(conn.pollInterval).toBe(5000);
	});
});
