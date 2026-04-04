<script lang="ts">
	let { label, value, sub = '' }: { label: string; value: string; sub?: string } = $props();
</script>

<div class="stat-card">
	<span class="label">{label}</span>
	<span class="value">{value}</span>
	{#if sub}
		<span class="sub">{sub}</span>
	{/if}
</div>

<style>
	.stat-card {
		background: var(--bg-surface);
		border: 1px solid var(--border-dim);
		border-radius: 12px;
		padding: 1.25rem 1.5rem;
		display: flex;
		flex-direction: column;
		gap: 0.25rem;
	}
	.label {
		font-family: var(--font-mono);
		font-size: 0.7rem;
		color: var(--text-muted);
		text-transform: uppercase;
		letter-spacing: 0.08em;
	}
	.value {
		font-family: var(--font-display);
		font-size: 1.75rem;
		font-weight: 400;
		color: var(--text-accent);
		line-height: 1.1;
	}
	.sub {
		font-family: var(--font-mono);
		font-size: 0.75rem;
		color: var(--text-secondary);
	}
</style>
