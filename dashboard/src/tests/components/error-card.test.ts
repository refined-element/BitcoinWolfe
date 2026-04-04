import { describe, it, expect, vi } from 'vitest';
import { render, screen, fireEvent } from '@testing-library/svelte';
import ErrorCard from '$lib/components/shared/ErrorCard.svelte';

describe('ErrorCard', () => {
	it('renders error message', () => {
		render(ErrorCard, { props: { message: 'Connection failed' } });
		expect(screen.getByText('Connection failed')).toBeInTheDocument();
	});

	it('has alert role', () => {
		render(ErrorCard, { props: { message: 'Something broke' } });
		expect(screen.getByRole('alert')).toBeInTheDocument();
	});

	it('renders error icon svg', () => {
		const { container } = render(ErrorCard, { props: { message: 'Error' } });
		const svg = container.querySelector('svg');
		expect(svg).toBeInTheDocument();
	});

	it('shows retry button when onretry is provided', () => {
		const handler = vi.fn();
		render(ErrorCard, { props: { message: 'Oops', onretry: handler } });
		expect(screen.getByText('Retry')).toBeInTheDocument();
	});

	it('does not show retry button when onretry is not provided', () => {
		render(ErrorCard, { props: { message: 'Oops' } });
		expect(screen.queryByText('Retry')).not.toBeInTheDocument();
	});

	it('fires onretry callback when retry button is clicked', async () => {
		const handler = vi.fn();
		render(ErrorCard, { props: { message: 'Try again', onretry: handler } });
		const btn = screen.getByText('Retry');
		await fireEvent.click(btn);
		expect(handler).toHaveBeenCalledTimes(1);
	});
});
