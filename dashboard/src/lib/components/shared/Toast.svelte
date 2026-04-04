<script lang="ts">
	import { getToasts } from '../../stores/toast.svelte';

	const toasts = getToasts();
</script>

{#if toasts.list.length > 0}
	<div class="toast-container">
		{#each toasts.list as toast (toast.id)}
			<div class="toast toast-{toast.type}">
				{toast.message}
			</div>
		{/each}
	</div>
{/if}

<style>
	.toast-container {
		position: fixed;
		bottom: 1.5rem;
		right: 1.5rem;
		z-index: 300;
		display: flex;
		flex-direction: column;
		gap: 0.5rem;
	}
	.toast {
		font-family: var(--font-mono);
		font-size: 0.8rem;
		padding: 0.75rem 1.25rem;
		border-radius: 8px;
		backdrop-filter: blur(12px);
		animation: slide-in 0.3s var(--ease-out-expo);
	}
	.toast-success {
		background: rgba(52, 211, 153, 0.15);
		border: 1px solid rgba(52, 211, 153, 0.3);
		color: var(--color-success);
	}
	.toast-error {
		background: rgba(239, 68, 68, 0.15);
		border: 1px solid rgba(239, 68, 68, 0.3);
		color: var(--color-error);
	}
	.toast-info {
		background: var(--orange-glow-strong);
		border: 1px solid var(--border-subtle);
		color: var(--text-accent);
	}
	@keyframes slide-in {
		from { opacity: 0; transform: translateY(10px); }
		to { opacity: 1; transform: translateY(0); }
	}
</style>
