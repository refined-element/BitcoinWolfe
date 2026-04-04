import { describe, it, expect, vi, beforeEach } from 'vitest';
import { createPoller } from '$lib/stores/polling.svelte';
import { setConnection } from '$lib/stores/connection.svelte';

describe('polling', () => {
	beforeEach(() => {
		vi.useFakeTimers();
		setConnection({ url: '', user: '', password: '', pollInterval: 3000 });
	});

	it('calls callback immediately when immediate=true', async () => {
		const cb = vi.fn().mockResolvedValue(undefined);
		createPoller(cb, true);
		await vi.advanceTimersByTimeAsync(0);
		expect(cb).toHaveBeenCalledTimes(1);
		vi.runAllTimers();
	});

	it('does not call immediately when immediate=false', async () => {
		const cb = vi.fn().mockResolvedValue(undefined);
		createPoller(cb, false);
		expect(cb).not.toHaveBeenCalled();
		vi.runAllTimers();
	});

	it('polls at configured interval', async () => {
		const cb = vi.fn().mockResolvedValue(undefined);
		createPoller(cb, true);

		// First call (immediate)
		await vi.advanceTimersByTimeAsync(0);
		expect(cb).toHaveBeenCalledTimes(1);

		// After interval
		await vi.advanceTimersByTimeAsync(3000);
		expect(cb).toHaveBeenCalledTimes(2);

		// After another interval
		await vi.advanceTimersByTimeAsync(3000);
		expect(cb).toHaveBeenCalledTimes(3);
		vi.runAllTimers();
	});

	it('stops polling when stop() is called', async () => {
		const cb = vi.fn().mockResolvedValue(undefined);
		const handle = createPoller(cb, true);

		await vi.advanceTimersByTimeAsync(0);
		expect(cb).toHaveBeenCalledTimes(1);

		handle.stop();
		await vi.advanceTimersByTimeAsync(10000);
		expect(cb).toHaveBeenCalledTimes(1); // no more calls
	});

	it('continues polling even if callback throws', async () => {
		let callCount = 0;
		const cb = vi.fn().mockImplementation(async () => {
			callCount++;
			if (callCount === 1) throw new Error('network error');
		});

		createPoller(cb, true);
		await vi.advanceTimersByTimeAsync(0); // first call (throws)
		await vi.advanceTimersByTimeAsync(3000); // second call (succeeds)
		expect(cb).toHaveBeenCalledTimes(2);
		vi.runAllTimers();
	});
});
