<script lang="ts">
	import type { Snippet } from 'svelte';
	import '../app.css';
	import Sidebar from '$lib/components/layout/Sidebar.svelte';
	import Toast from '$lib/components/shared/Toast.svelte';
	import { nodeStore } from '$lib/stores/node.svelte';
	import { sidebarStore } from '$lib/stores/sidebar.svelte';
	import { createPoller } from '$lib/stores/polling.svelte';
	import { onMount } from 'svelte';

	let { children }: { children: Snippet } = $props();

	let collapsed = $state(false);
	const node = nodeStore();
	const sidebar = sidebarStore();

	onMount(() => {
		const poller = createPoller(() => node.refresh());
		return () => poller.stop();
	});
</script>

<div class="app-shell" class:sidebar-collapsed={collapsed}>
	<Sidebar
		{collapsed}
		connected={node.connected}
		mobileOpen={sidebar.mobileOpen}
		onToggle={() => collapsed = !collapsed}
		onMobileClose={() => sidebar.close()}
	/>

	<main class="main-content">
		{@render children()}
	</main>
</div>

<Toast />

<style>
	.app-shell {
		display: flex;
		min-height: 100vh;
	}
	.main-content {
		flex: 1;
		margin-left: var(--sidebar-width);
		transition: margin-left 0.3s var(--ease-out-expo);
	}
	.sidebar-collapsed .main-content {
		margin-left: 64px;
	}

	@media (max-width: 768px) {
		.main-content {
			margin-left: 0;
		}
	}
</style>
