<script lang="ts">
	import type { Snippet } from 'svelte';

	let {
		children,
		variant = 'primary',
		disabled = false,
		loading = false,
		onclick,
		type = 'button',
		class: className = ''
	}: {
		children: Snippet;
		variant?: 'primary' | 'secondary' | 'danger';
		disabled?: boolean;
		loading?: boolean;
		onclick?: (e: MouseEvent) => void;
		type?: 'button' | 'submit';
		class?: string;
	} = $props();
</script>

<button
	class="btn btn-{variant} {className}"
	{disabled}
	{type}
	onclick={onclick}
>
	{#if loading}
		<span class="spinner"></span>
	{/if}
	{@render children()}
</button>

<style>
	.btn {
		font-family: var(--font-mono);
		font-size: 0.8rem;
		font-weight: 600;
		padding: 0.65rem 1.25rem;
		border: none;
		border-radius: 8px;
		cursor: pointer;
		transition: all 0.3s var(--ease-out-expo);
		display: inline-flex;
		align-items: center;
		gap: 0.5rem;
		white-space: nowrap;
	}
	.btn:disabled {
		opacity: 0.4;
		cursor: not-allowed;
	}
	.btn-primary {
		background: var(--text-accent);
		color: var(--bg-void);
	}
	.btn-primary:hover:not(:disabled) {
		background: var(--text-accent-bright);
		transform: translateY(-1px);
		box-shadow: 0 4px 20px rgba(255, 153, 0, 0.3);
	}
	.btn-secondary {
		background: transparent;
		color: var(--text-secondary);
		border: 1px solid var(--border-subtle);
	}
	.btn-secondary:hover:not(:disabled) {
		color: var(--text-primary);
		border-color: var(--border-accent);
		background: var(--orange-glow);
	}
	.btn-danger {
		background: transparent;
		color: var(--color-error);
		border: 1px solid rgba(239, 68, 68, 0.3);
	}
	.btn-danger:hover:not(:disabled) {
		background: rgba(239, 68, 68, 0.1);
		border-color: rgba(239, 68, 68, 0.5);
	}
	.spinner {
		width: 14px;
		height: 14px;
		border: 2px solid transparent;
		border-top-color: currentColor;
		border-radius: 50%;
		animation: spin 0.6s linear infinite;
	}
	@keyframes spin {
		to { transform: rotate(360deg); }
	}
</style>
