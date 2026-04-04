import { getConnection } from './connection.svelte';

type PollCallback = () => Promise<void>;

interface PollHandle {
	stop: () => void;
}

export function createPoller(callback: PollCallback, immediate = true): PollHandle {
	let timer: ReturnType<typeof setTimeout> | null = null;
	let stopped = false;

	async function tick() {
		if (stopped) return;
		try {
			await callback();
		} catch {
			// errors handled by caller
		}
		if (!stopped) {
			const interval = getConnection().pollInterval || 3000;
			timer = setTimeout(tick, interval);
		}
	}

	if (immediate) {
		tick();
	} else {
		const interval = getConnection().pollInterval || 3000;
		timer = setTimeout(tick, interval);
	}

	return {
		stop() {
			stopped = true;
			if (timer) clearTimeout(timer);
		}
	};
}
