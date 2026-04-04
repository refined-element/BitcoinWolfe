<script lang="ts">
	let { size = 24 }: { size?: number } = $props();
</script>

<svg class="spinner" width={size} height={size} viewBox="0 0 24 24" fill="none">
	<circle cx="12" cy="12" r="10" stroke="var(--border-subtle)" stroke-width="3" />
	<path d="M12 2a10 10 0 0 1 10 10" stroke="var(--text-accent)" stroke-width="3" stroke-linecap="round" />
</svg>

<style>
	.spinner {
		animation: spin 0.8s linear infinite;
	}
	@keyframes spin {
		to { transform: rotate(360deg); }
	}
</style>
