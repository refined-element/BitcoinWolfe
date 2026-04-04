<script lang="ts">
	import TopBar from '$lib/components/layout/TopBar.svelte';
	import StatCard from '$lib/components/shared/StatCard.svelte';
	import SyncProgress from '$lib/components/overview/SyncProgress.svelte';
	import Skeleton from '$lib/components/shared/Skeleton.svelte';
	import ErrorCard from '$lib/components/shared/ErrorCard.svelte';
	import { nodeStore } from '$lib/stores/node.svelte';
	import { sidebarStore } from '$lib/stores/sidebar.svelte';
	import { overviewStore } from '$lib/stores/overview.svelte';
	import { formatNumber, formatBytes, formatUptime, formatBtc, formatSats } from '$lib/utils/format';
	import { createPoller } from '$lib/stores/polling.svelte';
	import { onMount } from 'svelte';

	const node = nodeStore();
	const sidebar = sidebarStore();
	const overview = overviewStore();

	onMount(() => {
		const poller = createPoller(async () => {
			await node.refresh();
			await overview.refresh();
		});
		return () => poller.stop();
	});
</script>

<TopBar title="Overview" info={node.info} onMenuToggle={() => sidebar.toggle()} />

<div class="content">
	{#if !node.connected && !node.info}
		{#if node.error}
			<ErrorCard message={node.error} onretry={() => node.refresh()} />
			<p class="hint">Configure your connection in <a href="/settings">Settings</a></p>
		{:else}
			<div class="stats-grid">
				{#each Array(4) as _}
					<div class="skeleton-card">
						<Skeleton width="60%" height="0.7rem" />
						<Skeleton width="50%" height="1.75rem" />
						<Skeleton width="80%" height="0.75rem" />
					</div>
				{/each}
			</div>
		{/if}
	{:else if node.info}
		<div class="stats-grid">
			<SyncProgress
				blocks={node.info.blocks}
				headers={node.info.headers}
				syncing={node.info.syncing}
			/>

			<StatCard
				label="Block Height"
				value={formatNumber(node.info.blocks)}
				sub="{formatNumber(node.info.headers)} headers"
			/>
			<StatCard
				label="Mempool"
				value={formatNumber(node.info.mempool_size)}
				sub={node.mempool ? `Min fee: ${node.mempool.policy.min_fee_rate} sat/vB` : 'transactions'}
			/>
			<StatCard
				label="Peers"
				value={String(node.info.peers)}
				sub="connected"
			/>
			<StatCard
				label="Uptime"
				value={formatUptime(node.info.uptime_secs)}
				sub={node.info.user_agent}
			/>
			<StatCard
				label="Chain"
				value={node.info.chain}
				sub={node.info.syncing ? 'Syncing...' : 'Fully synced'}
			/>
			{#if node.mempool}
				<StatCard
					label="Mempool Size"
					value={formatBytes(node.mempool.bytes)}
					sub="full_rbf: {node.mempool.policy.full_rbf ? 'on' : 'off'}"
				/>
			{/if}
			{#if overview.walletAvailable && overview.walletBalance !== null}
				<StatCard
					label="Wallet Balance"
					value={formatBtc(overview.walletBalance)}
					sub="BTC"
				/>
			{/if}
			{#if overview.lnAvailable && overview.lnTotalCapacity !== null}
				<StatCard
					label="Lightning Capacity"
					value={formatSats(overview.lnTotalCapacity)}
					sub="sats across channels"
				/>
			{/if}
		</div>

		<div class="info-section">
			<div class="info-card">
				<h3>Best Block Hash</h3>
				<code class="hash">{node.info.best_block_hash}</code>
			</div>
		</div>
	{/if}
</div>

<style>
	.content {
		padding: 2rem;
		max-width: 1100px;
	}
	.stats-grid {
		display: grid;
		grid-template-columns: repeat(auto-fill, minmax(220px, 1fr));
		gap: 1rem;
	}
	.info-section {
		margin-top: 1.5rem;
	}
	.info-card {
		background: var(--bg-surface);
		border: 1px solid var(--border-dim);
		border-radius: 12px;
		padding: 1.25rem 1.5rem;
	}
	.info-card h3 {
		font-family: var(--font-mono);
		font-size: 0.7rem;
		color: var(--text-muted);
		text-transform: uppercase;
		letter-spacing: 0.08em;
		margin-bottom: 0.5rem;
	}
	.hash {
		font-family: var(--font-mono);
		font-size: 0.8rem;
		color: var(--text-secondary);
		word-break: break-all;
	}
	.skeleton-card {
		background: var(--bg-surface);
		border: 1px solid var(--border-dim);
		border-radius: 12px;
		padding: 1.25rem 1.5rem;
		display: flex;
		flex-direction: column;
		gap: 0.5rem;
	}
	.hint {
		color: var(--text-muted);
		font-family: var(--font-mono);
		font-size: 0.8rem;
		text-align: center;
		margin-top: 1rem;
	}
</style>
