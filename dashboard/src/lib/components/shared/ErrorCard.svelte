<script lang="ts">
	let {
		message,
		onretry
	}: {
		message: string;
		onretry?: () => void;
	} = $props();
</script>

<div class="error-card" role="alert">
	<div class="error-icon">
		<svg width="24" height="24" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round">
			<circle cx="12" cy="12" r="10" />
			<line x1="12" y1="8" x2="12" y2="12" />
			<line x1="12" y1="16" x2="12.01" y2="16" />
		</svg>
	</div>
	<p class="error-message">{message}</p>
	{#if onretry}
		<button class="retry-btn" onclick={onretry}>Retry</button>
	{/if}
</div>

<style>
	.error-card {
		display: flex;
		flex-direction: column;
		align-items: center;
		gap: 0.75rem;
		padding: 2rem;
		background: rgba(239, 68, 68, 0.04);
		border: 1px solid rgba(239, 68, 68, 0.15);
		border-radius: 12px;
	}
	.error-icon {
		color: var(--color-error);
		opacity: 0.8;
	}
	.error-message {
		font-family: var(--font-mono);
		font-size: 0.85rem;
		color: var(--color-error);
		text-align: center;
		line-height: 1.5;
	}
	.retry-btn {
		font-family: var(--font-mono);
		font-size: 0.78rem;
		font-weight: 600;
		padding: 0.5rem 1.25rem;
		border: 1px solid rgba(239, 68, 68, 0.3);
		border-radius: 8px;
		background: transparent;
		color: var(--color-error);
		cursor: pointer;
		transition: all 0.2s;
	}
	.retry-btn:hover {
		background: rgba(239, 68, 68, 0.1);
		border-color: rgba(239, 68, 68, 0.5);
	}
</style>
