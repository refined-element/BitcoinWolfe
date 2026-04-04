<script lang="ts">
	let {
		width = '100%',
		height = '1rem',
		rounded = false
	}: {
		width?: string;
		height?: string;
		rounded?: boolean;
	} = $props();
</script>

<div
	class="skeleton"
	class:rounded
	style="width: {width}; height: {height};"
	aria-hidden="true"
></div>

<style>
	.skeleton {
		background: var(--bg-raised);
		border-radius: 8px;
		position: relative;
		overflow: hidden;
	}
	.skeleton.rounded {
		border-radius: 9999px;
	}
	.skeleton::after {
		content: '';
		position: absolute;
		inset: 0;
		background: linear-gradient(
			90deg,
			transparent 0%,
			rgba(255, 153, 0, 0.04) 40%,
			rgba(255, 153, 0, 0.08) 50%,
			rgba(255, 153, 0, 0.04) 60%,
			transparent 100%
		);
		animation: skeleton-shimmer 1.8s ease-in-out infinite;
	}

	@keyframes skeleton-shimmer {
		0% { transform: translateX(-100%); }
		100% { transform: translateX(100%); }
	}
</style>
