import { describe, it, expect, vi } from 'vitest';
import { copyToClipboard } from '$lib/utils/clipboard';

describe('copyToClipboard', () => {
	it('copies text and returns true on success', async () => {
		const result = await copyToClipboard('hello');
		expect(result).toBe(true);
		expect(navigator.clipboard.writeText).toHaveBeenCalledWith('hello');
	});

	it('returns false on failure', async () => {
		vi.mocked(navigator.clipboard.writeText).mockRejectedValueOnce(new Error('denied'));
		const result = await copyToClipboard('fail');
		expect(result).toBe(false);
	});
});
