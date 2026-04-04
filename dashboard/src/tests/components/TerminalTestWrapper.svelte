<script lang="ts">
	import Terminal from '$lib/components/shared/Terminal.svelte';

	let {
		title = 'Terminal',
		bodyText = 'Terminal output here'
	}: {
		title?: string;
		bodyText?: string;
	} = $props();
</script>

<Terminal {title}>
	<p>{bodyText}</p>
</Terminal>
