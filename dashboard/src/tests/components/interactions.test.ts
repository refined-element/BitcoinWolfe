import { describe, it, expect, vi } from 'vitest';
import { render, screen, fireEvent } from '@testing-library/svelte';
import CopyButton from '$lib/components/shared/CopyButton.svelte';
import ModalTestWrapper from './ModalTestWrapper.svelte';
import TerminalTestWrapper from './TerminalTestWrapper.svelte';

describe('CopyButton interactions', () => {
	it('calls navigator.clipboard.writeText on click', async () => {
		render(CopyButton, { props: { text: 'hello world' } });

		const button = screen.getByTitle('Copy to clipboard');
		await fireEvent.click(button);

		expect(navigator.clipboard.writeText).toHaveBeenCalledWith('hello world');
	});

	it('shows check icon after successful copy', async () => {
		render(CopyButton, { props: { text: 'bc1qtest' } });

		const button = screen.getByTitle('Copy to clipboard');

		// Before copy: should have rect element (copy icon)
		const svgBefore = button.querySelector('svg');
		expect(svgBefore).toBeTruthy();
		expect(button.querySelector('rect')).toBeTruthy();

		await fireEvent.click(button);

		// After copy: should have polyline element (check icon)
		// Need to wait for the state update
		await vi.waitFor(() => {
			expect(button.querySelector('polyline')).toBeTruthy();
		});
	});

	it('copies different text values', async () => {
		render(CopyButton, { props: { text: '03abcdef1234' } });

		const button = screen.getByTitle('Copy to clipboard');
		await fireEvent.click(button);

		expect(navigator.clipboard.writeText).toHaveBeenCalledWith('03abcdef1234');
	});
});

describe('Modal interactions', () => {
	it('renders content when open=true', () => {
		render(ModalTestWrapper, { props: { open: true, title: 'Test Modal', bodyText: 'Hello from modal' } });

		expect(screen.getByText('Test Modal')).toBeInTheDocument();
		expect(screen.getByText('Hello from modal')).toBeInTheDocument();
	});

	it('does not render when open=false', () => {
		render(ModalTestWrapper, { props: { open: false, title: 'Hidden Modal', bodyText: 'Should not appear' } });

		expect(screen.queryByText('Hidden Modal')).not.toBeInTheDocument();
		expect(screen.queryByText('Should not appear')).not.toBeInTheDocument();
	});

	it('calls onclose when close button is clicked', async () => {
		const onclose = vi.fn();
		render(ModalTestWrapper, { props: { open: true, title: 'Close Me', onclose } });

		// The close button contains the × character
		const closeBtn = screen.getByText('\u00D7');
		await fireEvent.click(closeBtn);

		expect(onclose).toHaveBeenCalledTimes(1);
	});

	it('calls onclose when overlay (backdrop) is clicked', async () => {
		const onclose = vi.fn();
		const { container } = render(ModalTestWrapper, { props: { open: true, title: 'Backdrop Test', onclose } });

		const overlay = container.querySelector('.overlay');
		expect(overlay).toBeTruthy();

		// Click on the overlay itself (not the modal inside it)
		await fireEvent.click(overlay!);

		expect(onclose).toHaveBeenCalledTimes(1);
	});

	it('renders modal without title when title is empty', () => {
		render(ModalTestWrapper, { props: { open: true, title: '', bodyText: 'No title content' } });

		expect(screen.getByText('No title content')).toBeInTheDocument();
		expect(screen.queryByText('\u00D7')).not.toBeInTheDocument(); // no close button when no title
	});
});

describe('Terminal interactions', () => {
	it('renders title', () => {
		render(TerminalTestWrapper, { props: { title: 'Node Output' } });

		expect(screen.getByText('Node Output')).toBeInTheDocument();
	});

	it('renders children content', () => {
		render(TerminalTestWrapper, { props: { title: 'Terminal', bodyText: 'block 850000 validated' } });

		expect(screen.getByText('block 850000 validated')).toBeInTheDocument();
	});

	it('uses default title when none provided', () => {
		render(TerminalTestWrapper, { props: {} });

		expect(screen.getByText('Terminal')).toBeInTheDocument();
	});

	it('renders the terminal dots', () => {
		const { container } = render(TerminalTestWrapper, { props: { title: 'Test' } });

		const dots = container.querySelectorAll('.dot');
		expect(dots.length).toBe(3);
	});
});
