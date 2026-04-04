<script lang="ts">
	import { copyToClipboard } from '../../utils/clipboard';

	let { text }: { text: string } = $props();
	let copied = $state(false);

	async function handleCopy() {
		const ok = await copyToClipboard(text);
		if (ok) {
			copied = true;
			setTimeout(() => { copied = false; }, 2000);
		}
	}
</script>

<button class="copy-btn" onclick={handleCopy} title="Copy to clipboard">
	{#if copied}
		<svg width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2">
			<polyline points="20 6 9 17 4 12" />
		</svg>
	{:else}
		<svg width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2">
			<rect x="9" y="9" width="13" height="13" rx="2" ry="2" />
			<path d="M5 15H4a2 2 0 0 1-2-2V4a2 2 0 0 1 2-2h9a2 2 0 0 1 2 2v1" />
		</svg>
	{/if}
</button>

<style>
	.copy-btn {
		background: none;
		border: none;
		color: var(--text-muted);
		cursor: pointer;
		padding: 0.25rem;
		display: inline-flex;
		align-items: center;
		transition: color 0.2s;
	}
	.copy-btn:hover {
		color: var(--text-accent);
	}
</style>
