<script lang="ts">
	import type { Snippet } from 'svelte';

	let { title = 'Terminal', children }: { title?: string; children: Snippet } = $props();
</script>

<div class="terminal">
	<div class="terminal-bar">
		<div class="dot"></div>
		<div class="dot"></div>
		<div class="dot"></div>
		<span class="terminal-title">{title}</span>
	</div>
	<div class="terminal-body">
		{@render children()}
	</div>
</div>

<style>
	.terminal {
		background: var(--bg-surface);
		border: 1px solid var(--border-dim);
		border-radius: 12px;
		overflow: hidden;
	}
	.terminal-bar {
		display: flex;
		align-items: center;
		gap: 6px;
		padding: 12px 16px;
		background: var(--bg-raised);
		border-bottom: 1px solid var(--border-dim);
	}
	.dot {
		width: 10px;
		height: 10px;
		border-radius: 50%;
		background: var(--text-muted);
		opacity: 0.5;
	}
	.terminal-title {
		font-family: var(--font-mono);
		font-size: 0.7rem;
		color: var(--text-muted);
		margin-left: 8px;
	}
	.terminal-body {
		padding: 1.25rem 1.5rem;
		font-family: var(--font-mono);
		font-size: 0.82rem;
		line-height: 1.9;
		overflow-x: auto;
		white-space: pre-wrap;
		word-break: break-all;
	}
</style>
