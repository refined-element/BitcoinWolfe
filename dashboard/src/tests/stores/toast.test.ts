import { describe, it, expect, vi, beforeEach } from 'vitest';
import { showToast, getToasts } from '$lib/stores/toast.svelte';

describe('toast store', () => {
	beforeEach(() => {
		vi.useFakeTimers();
	});

	it('adds a toast', () => {
		const toasts = getToasts();
		showToast('Hello', 'info');
		expect(toasts.list.length).toBe(1);
		expect(toasts.list[0].message).toBe('Hello');
		expect(toasts.list[0].type).toBe('info');
		vi.runAllTimers();
	});

	it('auto-removes after 4 seconds', () => {
		const toasts = getToasts();
		showToast('Temporary', 'success');
		expect(toasts.list.length).toBeGreaterThanOrEqual(1);
		vi.advanceTimersByTime(4100);
		expect(toasts.list.filter(t => t.message === 'Temporary').length).toBe(0);
	});

	it('supports different types', () => {
		showToast('Error!', 'error');
		const toasts = getToasts();
		const errorToast = toasts.list.find(t => t.message === 'Error!');
		expect(errorToast?.type).toBe('error');
		vi.runAllTimers();
	});

	it('assigns unique IDs', () => {
		showToast('First', 'info');
		showToast('Second', 'info');
		const toasts = getToasts();
		const ids = toasts.list.map(t => t.id);
		expect(new Set(ids).size).toBe(ids.length);
		vi.runAllTimers();
	});
});
