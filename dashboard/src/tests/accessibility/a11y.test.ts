import { describe, it, expect } from 'vitest';
import { render, screen } from '@testing-library/svelte';
import ErrorCard from '$lib/components/shared/ErrorCard.svelte';
import Sidebar from '$lib/components/layout/Sidebar.svelte';
import TopBar from '$lib/components/layout/TopBar.svelte';
import Spinner from '$lib/components/shared/Spinner.svelte';
import Badge from '$lib/components/shared/Badge.svelte';

describe('Accessibility', () => {
	describe('ErrorCard', () => {
		it('has role="alert"', () => {
			render(ErrorCard, { props: { message: 'Something went wrong' } });

			const alert = screen.getByRole('alert');
			expect(alert).toBeInTheDocument();
		});

		it('role="alert" contains the error message', () => {
			render(ErrorCard, { props: { message: 'Connection refused' } });

			const alert = screen.getByRole('alert');
			expect(alert).toHaveTextContent('Connection refused');
		});
	});

	describe('Sidebar navigation', () => {
		it('nav items are proper links with href attributes', () => {
			render(Sidebar, { props: { collapsed: false, connected: true, onToggle: () => {} } });

			const links = screen.getAllByRole('link');
			// Should have 7 links: logo link + 6 nav items
			expect(links.length).toBeGreaterThanOrEqual(7);

			// Each nav link should have a valid href
			const navHrefs = links.map(l => l.getAttribute('href')).filter(Boolean);
			expect(navHrefs).toContain('/');
			expect(navHrefs).toContain('/wallet');
			expect(navHrefs).toContain('/lightning');
			expect(navHrefs).toContain('/nostr');
			expect(navHrefs).toContain('/peers');
			expect(navHrefs).toContain('/settings');
		});

		it('collapse button has a title attribute', () => {
			const { container } = render(Sidebar, { props: { collapsed: false, connected: true, onToggle: () => {} } });

			const collapseBtn = container.querySelector('.collapse-btn');
			expect(collapseBtn).toBeTruthy();
			expect(collapseBtn?.getAttribute('title')).toBe('Collapse sidebar');
		});

		it('collapse button title changes when collapsed', () => {
			const { container } = render(Sidebar, { props: { collapsed: true, connected: true, onToggle: () => {} } });

			const collapseBtn = container.querySelector('.collapse-btn');
			expect(collapseBtn?.getAttribute('title')).toBe('Expand sidebar');
		});
	});

	describe('TopBar hamburger menu', () => {
		it('hamburger menu button has aria-label', () => {
			render(TopBar, { props: { title: 'Test', info: null, onMenuToggle: () => {} } });

			const menuBtn = screen.getByLabelText('Toggle menu');
			expect(menuBtn).toBeInTheDocument();
			expect(menuBtn.getAttribute('aria-label')).toBe('Toggle menu');
		});

		it('hamburger menu button is not rendered when onMenuToggle is not provided', () => {
			render(TopBar, { props: { title: 'Test', info: null } });

			expect(screen.queryByLabelText('Toggle menu')).not.toBeInTheDocument();
		});
	});

	describe('Spinner', () => {
		it('SVG element is present as visual indicator', () => {
			const { container } = render(Spinner, { props: {} });

			const svg = container.querySelector('svg.spinner');
			expect(svg).toBeTruthy();
			expect(svg?.getAttribute('viewBox')).toBe('0 0 24 24');
		});

		it('renders with custom size', () => {
			const { container } = render(Spinner, { props: { size: 32 } });

			const svg = container.querySelector('svg.spinner');
			expect(svg?.getAttribute('width')).toBe('32');
			expect(svg?.getAttribute('height')).toBe('32');
		});
	});

	describe('Badge', () => {
		it('text is visible (not hidden)', () => {
			render(Badge, { props: { text: 'mainnet' } });

			const badge = screen.getByText('mainnet');
			expect(badge).toBeInTheDocument();
			expect(badge).toBeVisible();
		});

		it('text is visible for each variant', () => {
			const { unmount: u1 } = render(Badge, { props: { text: 'Success', variant: 'success' } });
			expect(screen.getByText('Success')).toBeVisible();
			u1();

			const { unmount: u2 } = render(Badge, { props: { text: 'Warning', variant: 'warning' } });
			expect(screen.getByText('Warning')).toBeVisible();
			u2();

			const { unmount: u3 } = render(Badge, { props: { text: 'Error', variant: 'error' } });
			expect(screen.getByText('Error')).toBeVisible();
			u3();

			render(Badge, { props: { text: 'Accent', variant: 'accent' } });
			expect(screen.getByText('Accent')).toBeVisible();
		});
	});
});
