<script lang="ts">
	import type { Snippet } from 'svelte';

	let {
		open = false,
		title = '',
		children,
		onclose
	}: {
		open: boolean;
		title?: string;
		children: Snippet;
		onclose: () => void;
	} = $props();

	function handleKeydown(e: KeyboardEvent) {
		if (e.key === 'Escape') onclose();
	}
</script>

{#if open}
	<!-- svelte-ignore a11y_no_static_element_interactions -->
	<div class="overlay" onclick={onclose} onkeydown={handleKeydown}>
		<!-- svelte-ignore a11y_no_static_element_interactions -->
		<div class="modal" onclick={(e) => e.stopPropagation()} onkeydown={handleKeydown}>
			{#if title}
				<div class="modal-header">
					<h3>{title}</h3>
					<button class="close-btn" onclick={onclose}>&times;</button>
				</div>
			{/if}
			<div class="modal-body">
				{@render children()}
			</div>
		</div>
	</div>
{/if}

<style>
	.overlay {
		position: fixed;
		inset: 0;
		background: rgba(0, 0, 0, 0.7);
		backdrop-filter: blur(4px);
		z-index: 200;
		display: flex;
		align-items: center;
		justify-content: center;
		padding: 1rem;
	}
	.modal {
		background: var(--bg-surface);
		border: 1px solid var(--border-subtle);
		border-radius: 16px;
		max-width: 500px;
		width: 100%;
		max-height: 90vh;
		overflow-y: auto;
	}
	.modal-header {
		display: flex;
		align-items: center;
		justify-content: space-between;
		padding: 1.25rem 1.5rem;
		border-bottom: 1px solid var(--border-dim);
	}
	.modal-header h3 {
		font-family: var(--font-display);
		font-size: 1.3rem;
		font-weight: 400;
	}
	.close-btn {
		background: none;
		border: none;
		color: var(--text-muted);
		font-size: 1.5rem;
		cursor: pointer;
		padding: 0;
		line-height: 1;
	}
	.close-btn:hover { color: var(--text-primary); }
	.modal-body {
		padding: 1.5rem;
	}
</style>
