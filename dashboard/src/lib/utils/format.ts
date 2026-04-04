export function formatBtc(sats: number): string {
	return sats.toFixed(8);
}

export function formatSats(sats: number): string {
	return new Intl.NumberFormat().format(sats);
}

export function formatNumber(n: number): string {
	return new Intl.NumberFormat().format(n);
}

export function formatBytes(bytes: number): string {
	if (bytes < 1024) return `${bytes} B`;
	if (bytes < 1024 * 1024) return `${(bytes / 1024).toFixed(1)} KB`;
	if (bytes < 1024 * 1024 * 1024) return `${(bytes / (1024 * 1024)).toFixed(1)} MB`;
	return `${(bytes / (1024 * 1024 * 1024)).toFixed(2)} GB`;
}

export function formatUptime(secs: number): string {
	const d = Math.floor(secs / 86400);
	const h = Math.floor((secs % 86400) / 3600);
	const m = Math.floor((secs % 3600) / 60);
	if (d > 0) return `${d}d ${h}h`;
	if (h > 0) return `${h}h ${m}m`;
	return `${m}m`;
}

export function truncateMiddle(str: string, startLen = 8, endLen = 8): string {
	if (str.length <= startLen + endLen + 3) return str;
	return `${str.slice(0, startLen)}...${str.slice(-endLen)}`;
}

export function formatPercent(n: number): string {
	return `${(n * 100).toFixed(1)}%`;
}
