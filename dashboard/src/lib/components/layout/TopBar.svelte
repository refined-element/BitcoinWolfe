<script lang="ts">
	import Badge from '../shared/Badge.svelte';
	import { formatNumber } from '../../utils/format';
	import type { NodeInfo } from '../../api/types';

	let { title, info, onMenuToggle }: { title: string; info: NodeInfo | null; onMenuToggle?: () => void } = $props();
</script>

<header class="topbar">
	<div class="topbar-left">
		{#if onMenuToggle}
			<button class="menu-btn" onclick={onMenuToggle} aria-label="Toggle menu">
				<svg width="20" height="20" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round">
					<line x1="3" y1="6" x2="21" y2="6" />
					<line x1="3" y1="12" x2="21" y2="12" />
					<line x1="3" y1="18" x2="21" y2="18" />
				</svg>
			</button>
		{/if}
		<h1 class="page-title">{title}</h1>
	</div>

	{#if info}
		<div class="status-badges">
			<Badge text={info.chain} variant="accent" />
			<Badge text="Block {formatNumber(info.blocks)}" variant="default" />
			{#if info.syncing}
				<Badge text="Syncing" variant="warning" />
			{:else}
				<Badge text="Synced" variant="success" />
			{/if}
			<Badge text="{info.peers} peers" variant="default" />
		</div>
	{/if}
</header>

<style>
	.topbar {
		height: var(--topbar-height);
		display: flex;
		align-items: center;
		justify-content: space-between;
		padding: 0 2rem;
		border-bottom: 1px solid var(--border-dim);
		background: var(--bg-void);
		position: sticky;
		top: 0;
		z-index: 40;
	}
	.topbar-left {
		display: flex;
		align-items: center;
		gap: 0.75rem;
	}
	.menu-btn {
		display: none;
		background: none;
		border: none;
		color: var(--text-secondary);
		cursor: pointer;
		padding: 0.25rem;
		transition: color 0.2s;
	}
	.menu-btn:hover { color: var(--text-accent); }
	.page-title {
		font-family: var(--font-display);
		font-size: 1.4rem;
		font-weight: 400;
		color: var(--text-primary);
	}
	.status-badges {
		display: flex;
		gap: 0.5rem;
		align-items: center;
	}
	@media (max-width: 768px) {
		.menu-btn {
			display: flex;
		}
	}
	@media (max-width: 640px) {
		.status-badges {
			display: none;
		}
	}
</style>
