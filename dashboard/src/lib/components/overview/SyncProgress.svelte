<script lang="ts">
	import { formatNumber, formatPercent } from '../../utils/format';

	let { blocks, headers, syncing }: { blocks: number; headers: number; syncing: boolean } = $props();

	let progress = $derived(headers > 0 ? blocks / headers : 0);
</script>

{#if syncing}
	<div class="sync-progress">
		<div class="sync-header">
			<span class="sync-label">Sync Progress</span>
			<span class="sync-pct">{formatPercent(progress)}</span>
		</div>
		<div class="progress-bar">
			<div class="progress-fill" style="width: {progress * 100}%"></div>
		</div>
		<div class="sync-detail">
			<span>{formatNumber(blocks)} / {formatNumber(headers)} blocks</span>
		</div>
	</div>
{/if}

<style>
	.sync-progress {
		background: var(--bg-surface);
		border: 1px solid var(--border-dim);
		border-radius: 12px;
		padding: 1.25rem 1.5rem;
		grid-column: 1 / -1;
	}
	.sync-header {
		display: flex;
		justify-content: space-between;
		align-items: center;
		margin-bottom: 0.75rem;
	}
	.sync-label {
		font-family: var(--font-mono);
		font-size: 0.75rem;
		color: var(--text-muted);
		text-transform: uppercase;
		letter-spacing: 0.08em;
	}
	.sync-pct {
		font-family: var(--font-display);
		font-size: 1.25rem;
		color: var(--text-accent);
	}
	.progress-bar {
		height: 6px;
		background: var(--bg-raised);
		border-radius: 3px;
		overflow: hidden;
	}
	.progress-fill {
		height: 100%;
		background: linear-gradient(90deg, var(--text-accent), var(--text-accent-bright));
		border-radius: 3px;
		transition: width 0.5s var(--ease-out-expo);
	}
	.sync-detail {
		margin-top: 0.5rem;
		font-family: var(--font-mono);
		font-size: 0.7rem;
		color: var(--text-secondary);
	}
</style>
