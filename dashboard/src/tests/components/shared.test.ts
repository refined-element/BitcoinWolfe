import { describe, it, expect } from 'vitest';
import { render, screen } from '@testing-library/svelte';
import Badge from '$lib/components/shared/Badge.svelte';
import StatCard from '$lib/components/shared/StatCard.svelte';
import Spinner from '$lib/components/shared/Spinner.svelte';

describe('Badge', () => {
	it('renders text', () => {
		render(Badge, { props: { text: 'mainnet' } });
		expect(screen.getByText('mainnet')).toBeInTheDocument();
	});

	it('applies variant class', () => {
		render(Badge, { props: { text: 'Synced', variant: 'success' } });
		const el = screen.getByText('Synced');
		expect(el.className).toContain('badge-success');
	});

	it('defaults to default variant', () => {
		render(Badge, { props: { text: 'Test' } });
		const el = screen.getByText('Test');
		expect(el.className).toContain('badge-default');
	});
});

describe('StatCard', () => {
	it('renders label and value', () => {
		render(StatCard, { props: { label: 'Block Height', value: '850,000' } });
		expect(screen.getByText('Block Height')).toBeInTheDocument();
		expect(screen.getByText('850,000')).toBeInTheDocument();
	});

	it('renders sub text when provided', () => {
		render(StatCard, { props: { label: 'Peers', value: '8', sub: 'connected' } });
		expect(screen.getByText('connected')).toBeInTheDocument();
	});

	it('does not render sub when empty', () => {
		render(StatCard, { props: { label: 'Count', value: '5' } });
		const container = screen.getByText('Count').closest('.stat-card');
		expect(container?.querySelectorAll('.sub').length).toBe(0);
	});
});

describe('Spinner', () => {
	it('renders svg element', () => {
		const { container } = render(Spinner, { props: { size: 32 } });
		const svg = container.querySelector('svg');
		expect(svg).toBeInTheDocument();
		expect(svg?.getAttribute('width')).toBe('32');
	});

	it('defaults to size 24', () => {
		const { container } = render(Spinner);
		const svg = container.querySelector('svg');
		expect(svg?.getAttribute('width')).toBe('24');
	});
});
