import { describe, it, expect } from 'vitest';
import { render, screen } from '@testing-library/svelte';
import TopBar from '$lib/components/layout/TopBar.svelte';

describe('TopBar', () => {
	it('renders page title', () => {
		render(TopBar, { props: { title: 'Overview', info: null } });
		expect(screen.getByText('Overview')).toBeInTheDocument();
	});

	it('renders status badges when info provided', () => {
		const info = {
			version: '0.1.0', user_agent: '/BitcoinWolfe/', chain: 'mainnet',
			blocks: 850000, headers: 850000, best_block_hash: 'abc',
			mempool_size: 10, peers: 8, uptime_secs: 3600, syncing: false,
		};
		render(TopBar, { props: { title: 'Overview', info } });
		expect(screen.getByText('mainnet')).toBeInTheDocument();
		expect(screen.getByText('Synced')).toBeInTheDocument();
		expect(screen.getByText('8 peers')).toBeInTheDocument();
	});

	it('shows Syncing badge when syncing', () => {
		const info = {
			version: '0.1.0', user_agent: '/BitcoinWolfe/', chain: 'mainnet',
			blocks: 500000, headers: 850000, best_block_hash: 'abc',
			mempool_size: 0, peers: 4, uptime_secs: 100, syncing: true,
		};
		render(TopBar, { props: { title: 'Dashboard', info } });
		expect(screen.getByText('Syncing')).toBeInTheDocument();
	});

	it('does not render badges when info is null', () => {
		render(TopBar, { props: { title: 'Test', info: null } });
		expect(screen.queryByText('mainnet')).not.toBeInTheDocument();
	});
});
