import { describe, it, expect } from 'vitest';
import { render, screen } from '@testing-library/svelte';
import SyncProgress from '$lib/components/overview/SyncProgress.svelte';

describe('SyncProgress', () => {
	it('renders when syncing', () => {
		render(SyncProgress, { props: { blocks: 500000, headers: 850000, syncing: true } });
		expect(screen.getByText('Sync Progress')).toBeInTheDocument();
	});

	it('shows correct percentage', () => {
		render(SyncProgress, { props: { blocks: 425000, headers: 850000, syncing: true } });
		expect(screen.getByText('50.0%')).toBeInTheDocument();
	});

	it('shows block counts', () => {
		render(SyncProgress, { props: { blocks: 500000, headers: 850000, syncing: true } });
		expect(screen.getByText(/500,000/)).toBeInTheDocument();
		expect(screen.getByText(/850,000/)).toBeInTheDocument();
	});

	it('does not render when not syncing', () => {
		const { container } = render(SyncProgress, { props: { blocks: 850000, headers: 850000, syncing: false } });
		expect(container.querySelector('.sync-progress')).not.toBeInTheDocument();
	});

	it('handles zero headers gracefully', () => {
		render(SyncProgress, { props: { blocks: 0, headers: 0, syncing: true } });
		expect(screen.getByText('0.0%')).toBeInTheDocument();
	});
});
