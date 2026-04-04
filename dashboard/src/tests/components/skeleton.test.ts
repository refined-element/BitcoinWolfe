import { describe, it, expect } from 'vitest';
import { render } from '@testing-library/svelte';
import Skeleton from '$lib/components/shared/Skeleton.svelte';

describe('Skeleton', () => {
	it('renders with default dimensions', () => {
		const { container } = render(Skeleton);
		const el = container.querySelector('.skeleton');
		expect(el).toBeInTheDocument();
		expect(el?.getAttribute('style')).toContain('width: 100%');
		expect(el?.getAttribute('style')).toContain('height: 1rem');
	});

	it('renders with custom width and height', () => {
		const { container } = render(Skeleton, { props: { width: '200px', height: '2rem' } });
		const el = container.querySelector('.skeleton');
		expect(el?.getAttribute('style')).toContain('width: 200px');
		expect(el?.getAttribute('style')).toContain('height: 2rem');
	});

	it('applies rounded class when rounded prop is true', () => {
		const { container } = render(Skeleton, { props: { rounded: true } });
		const el = container.querySelector('.skeleton');
		expect(el?.classList.contains('rounded')).toBe(true);
	});

	it('does not apply rounded class by default', () => {
		const { container } = render(Skeleton);
		const el = container.querySelector('.skeleton');
		expect(el?.classList.contains('rounded')).toBe(false);
	});

	it('has aria-hidden attribute', () => {
		const { container } = render(Skeleton);
		const el = container.querySelector('.skeleton');
		expect(el?.getAttribute('aria-hidden')).toBe('true');
	});
});
