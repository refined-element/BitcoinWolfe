import { describe, it, expect } from 'vitest';
import { formatBtc, formatSats, formatNumber, formatBytes, formatUptime, truncateMiddle, formatPercent } from '$lib/utils/format';

describe('formatBtc', () => {
	it('formats zero', () => {
		expect(formatBtc(0)).toBe('0.00000000');
	});

	it('formats whole BTC', () => {
		expect(formatBtc(1)).toBe('1.00000000');
	});

	it('formats fractional BTC', () => {
		expect(formatBtc(0.00012345)).toBe('0.00012345');
	});

	it('formats large amounts', () => {
		expect(formatBtc(21000000)).toBe('21000000.00000000');
	});
});

describe('formatSats', () => {
	it('formats small numbers', () => {
		expect(formatSats(100)).toBe('100');
	});

	it('adds thousands separators', () => {
		expect(formatSats(1000000)).toMatch(/1.*000.*000/);
	});
});

describe('formatNumber', () => {
	it('formats integers', () => {
		expect(formatNumber(0)).toBe('0');
	});

	it('adds separators for large numbers', () => {
		expect(formatNumber(850000)).toMatch(/850.*000/);
	});
});

describe('formatBytes', () => {
	it('formats bytes', () => {
		expect(formatBytes(500)).toBe('500 B');
	});

	it('formats kilobytes', () => {
		expect(formatBytes(2048)).toBe('2.0 KB');
	});

	it('formats megabytes', () => {
		expect(formatBytes(5 * 1024 * 1024)).toBe('5.0 MB');
	});

	it('formats gigabytes', () => {
		expect(formatBytes(2.5 * 1024 * 1024 * 1024)).toBe('2.50 GB');
	});
});

describe('formatUptime', () => {
	it('formats minutes only', () => {
		expect(formatUptime(300)).toBe('5m');
	});

	it('formats hours and minutes', () => {
		expect(formatUptime(3700)).toBe('1h 1m');
	});

	it('formats days and hours', () => {
		expect(formatUptime(90000)).toBe('1d 1h');
	});

	it('handles zero', () => {
		expect(formatUptime(0)).toBe('0m');
	});
});

describe('truncateMiddle', () => {
	it('returns short strings unchanged', () => {
		expect(truncateMiddle('short')).toBe('short');
	});

	it('truncates long strings', () => {
		const long = '0123456789abcdef0123456789abcdef';
		const result = truncateMiddle(long, 8, 8);
		expect(result).toContain('...');
		expect(result.startsWith('01234567')).toBe(true);
		expect(result.endsWith('9abcdef')).toBe(true);
		expect(result.length).toBeLessThan(long.length);
	});

	it('respects custom lengths', () => {
		const result = truncateMiddle('abcdefghijklmnopqrstuvwxyz', 4, 4);
		expect(result).toBe('abcd...wxyz');
	});
});

describe('formatPercent', () => {
	it('formats decimal as percent', () => {
		expect(formatPercent(0.5)).toBe('50.0%');
	});

	it('formats 100%', () => {
		expect(formatPercent(1)).toBe('100.0%');
	});

	it('formats 0%', () => {
		expect(formatPercent(0)).toBe('0.0%');
	});

	it('formats fractional percent', () => {
		expect(formatPercent(0.9537)).toBe('95.4%');
	});
});
