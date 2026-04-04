<script lang="ts">
	let { text, variant = 'default' }: { text: string; variant?: 'default' | 'success' | 'warning' | 'error' | 'accent' } = $props();
</script>

<span class="badge badge-{variant}">{text}</span>

<style>
	.badge {
		font-family: var(--font-mono);
		font-size: 0.65rem;
		font-weight: 500;
		padding: 0.2rem 0.55rem;
		border-radius: 100px;
		text-transform: uppercase;
		letter-spacing: 0.04em;
		white-space: nowrap;
	}
	.badge-default {
		color: var(--text-secondary);
		background: var(--bg-raised);
		border: 1px solid var(--border-dim);
	}
	.badge-success {
		color: var(--color-success);
		background: rgba(52, 211, 153, 0.08);
		border: 1px solid rgba(52, 211, 153, 0.2);
	}
	.badge-warning {
		color: var(--color-warning);
		background: rgba(245, 158, 11, 0.08);
		border: 1px solid rgba(245, 158, 11, 0.2);
	}
	.badge-error {
		color: var(--color-error);
		background: rgba(239, 68, 68, 0.08);
		border: 1px solid rgba(239, 68, 68, 0.2);
	}
	.badge-accent {
		color: var(--text-accent);
		background: var(--orange-glow);
		border: 1px solid var(--border-subtle);
	}
</style>
