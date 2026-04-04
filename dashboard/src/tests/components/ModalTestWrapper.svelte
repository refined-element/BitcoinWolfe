<script lang="ts">
	import Modal from '$lib/components/shared/Modal.svelte';

	let {
		open = false,
		title = '',
		bodyText = 'Modal body content',
		onclose = () => {}
	}: {
		open?: boolean;
		title?: string;
		bodyText?: string;
		onclose?: () => void;
	} = $props();
</script>

<Modal {open} {title} {onclose}>
	<p>{bodyText}</p>
</Modal>
