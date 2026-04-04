<script lang="ts">
	import Button from '$lib/components/shared/Button.svelte';

	let {
		text = 'Button',
		variant = 'primary',
		disabled = false,
		loading = false,
		onclick = () => {}
	}: {
		text?: string;
		variant?: 'primary' | 'secondary' | 'danger';
		disabled?: boolean;
		loading?: boolean;
		onclick?: (e: MouseEvent) => void;
	} = $props();
</script>

<Button {variant} {disabled} {loading} {onclick}>
	{text}
</Button>
