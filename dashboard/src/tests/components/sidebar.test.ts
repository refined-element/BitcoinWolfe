import { describe, it, expect } from 'vitest';
import { render, screen } from '@testing-library/svelte';
import Sidebar from '$lib/components/layout/Sidebar.svelte';

describe('Sidebar', () => {
	it('renders all nav items', () => {
		render(Sidebar, { props: { collapsed: false, connected: true, onToggle: () => {} } });
		expect(screen.getByText('Overview')).toBeInTheDocument();
		expect(screen.getByText('Wallet')).toBeInTheDocument();
		expect(screen.getByText('Lightning')).toBeInTheDocument();
		expect(screen.getByText('Nostr')).toBeInTheDocument();
		expect(screen.getByText('Peers')).toBeInTheDocument();
		expect(screen.getByText('Settings')).toBeInTheDocument();
	});

	it('renders logo text', () => {
		render(Sidebar, { props: { collapsed: false, connected: false, onToggle: () => {} } });
		expect(screen.getByText('BitcoinWolfe')).toBeInTheDocument();
	});

	it('shows Connected status when connected', () => {
		render(Sidebar, { props: { collapsed: false, connected: true, onToggle: () => {} } });
		expect(screen.getByText('Connected')).toBeInTheDocument();
	});

	it('shows Disconnected status when not connected', () => {
		render(Sidebar, { props: { collapsed: false, connected: false, onToggle: () => {} } });
		expect(screen.getByText('Disconnected')).toBeInTheDocument();
	});

	it('has correct nav links', () => {
		render(Sidebar, { props: { collapsed: false, connected: true, onToggle: () => {} } });
		const links = screen.getAllByRole('link');
		const hrefs = links.map(l => l.getAttribute('href'));
		expect(hrefs).toContain('/');
		expect(hrefs).toContain('/wallet');
		expect(hrefs).toContain('/lightning');
		expect(hrefs).toContain('/nostr');
		expect(hrefs).toContain('/peers');
		expect(hrefs).toContain('/settings');
	});
});
