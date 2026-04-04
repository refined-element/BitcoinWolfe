<script lang="ts">
	import type { Snippet } from 'svelte';

	let { children, class: className = '' }: { children: Snippet; class?: string } = $props();
</script>

<div class="card {className}">
	{@render children()}
</div>

<style>
	.card {
		background: var(--bg-surface);
		border: 1px solid var(--border-dim);
		border-radius: 12px;
		padding: 1.5rem;
	}
</style>
