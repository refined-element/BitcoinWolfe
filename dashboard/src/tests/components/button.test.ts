import { describe, it, expect, vi } from 'vitest';
import { render, screen, fireEvent } from '@testing-library/svelte';
import Button from '$lib/components/shared/Button.svelte';
import ButtonTestWrapper from './ButtonTestWrapper.svelte';

describe('Button', () => {
	it('renders children text', () => {
		render(ButtonTestWrapper, { props: { text: 'Click me', variant: 'primary' } });
		expect(screen.getByText('Click me')).toBeInTheDocument();
	});

	it('applies variant class', () => {
		render(ButtonTestWrapper, { props: { text: 'Go', variant: 'secondary' } });
		const btn = screen.getByRole('button');
		expect(btn.className).toContain('btn-secondary');
	});

	it('is disabled when disabled prop set', () => {
		render(ButtonTestWrapper, { props: { text: 'No', variant: 'primary', disabled: true } });
		const btn = screen.getByRole('button');
		expect(btn).toBeDisabled();
	});

	it('shows spinner when loading', () => {
		render(ButtonTestWrapper, { props: { text: 'Wait', variant: 'primary', loading: true } });
		const btn = screen.getByRole('button');
		expect(btn.querySelector('.spinner')).toBeInTheDocument();
	});

	it('fires onclick handler', async () => {
		const handler = vi.fn();
		render(ButtonTestWrapper, { props: { text: 'Fire', variant: 'primary', onclick: handler } });
		const btn = screen.getByRole('button');
		await fireEvent.click(btn);
		expect(handler).toHaveBeenCalledTimes(1);
	});

	it('applies danger variant', () => {
		render(ButtonTestWrapper, { props: { text: 'Delete', variant: 'danger' } });
		const btn = screen.getByRole('button');
		expect(btn.className).toContain('btn-danger');
	});
});
