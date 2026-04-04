<script lang="ts">
	import TopBar from '$lib/components/layout/TopBar.svelte';
	import StatCard from '$lib/components/shared/StatCard.svelte';
	import Card from '$lib/components/shared/Card.svelte';
	import Badge from '$lib/components/shared/Badge.svelte';
	import Skeleton from '$lib/components/shared/Skeleton.svelte';
	import ErrorCard from '$lib/components/shared/ErrorCard.svelte';
	import { nodeStore } from '$lib/stores/node.svelte';
	import { peersStore } from '$lib/stores/peers.svelte';
	import { sidebarStore } from '$lib/stores/sidebar.svelte';
	import { createPoller } from '$lib/stores/polling.svelte';
	import { formatNumber } from '$lib/utils/format';
	import { onMount } from 'svelte';

	const node = nodeStore();
	const peers = peersStore();
	const sidebar = sidebarStore();

	type SortKey = 'addr' | 'user_agent' | 'version' | 'inbound' | 'v2_transport' | 'start_height';
	let sortKey = $state<SortKey>('addr');
	let sortAsc = $state(true);

	onMount(() => {
		const poller = createPoller(() => peers.refresh());
		return () => poller.stop();
	});

	let sorted = $derived.by(() => {
		const list = [...peers.list];
		list.sort((a, b) => {
			const av = a[sortKey];
			const bv = b[sortKey];
			if (typeof av === 'string' && typeof bv === 'string') {
				return sortAsc ? av.localeCompare(bv) : bv.localeCompare(av);
			}
			if (typeof av === 'number' && typeof bv === 'number') {
				return sortAsc ? av - bv : bv - av;
			}
			if (typeof av === 'boolean' && typeof bv === 'boolean') {
				return sortAsc ? (av === bv ? 0 : av ? -1 : 1) : (av === bv ? 0 : av ? 1 : -1);
			}
			return 0;
		});
		return list;
	});

	let inboundCount = $derived(peers.list.filter(p => p.inbound).length);
	let outboundCount = $derived(peers.list.filter(p => !p.inbound).length);
	let v2Count = $derived(peers.list.filter(p => p.v2_transport).length);

	function setSort(key: SortKey) {
		if (sortKey === key) {
			sortAsc = !sortAsc;
		} else {
			sortKey = key;
			sortAsc = true;
		}
	}

	function sortIndicator(key: SortKey): string {
		if (sortKey !== key) return '';
		return sortAsc ? ' \u25B2' : ' \u25BC';
	}
</script>

<TopBar title="Peers" info={node.info} onMenuToggle={() => sidebar.toggle()} />

<div class="content">
	{#if peers.error}
		<ErrorCard message={peers.error} onretry={() => peers.refresh()} />
	{:else if !peers.loaded}
		<!-- Skeleton loading state -->
		<div class="stats-grid">
			{#each Array(4) as _}
				<div class="skeleton-card">
					<Skeleton width="60%" height="0.7rem" />
					<Skeleton width="40%" height="1.75rem" />
					<Skeleton width="80%" height="0.75rem" />
				</div>
			{/each}
		</div>
		<Card>
			<div class="skeleton-table">
				{#each Array(5) as _}
					<div class="skeleton-row">
						<Skeleton width="30%" height="0.85rem" />
						<Skeleton width="25%" height="0.85rem" />
						<Skeleton width="10%" height="0.85rem" />
						<Skeleton width="12%" height="0.85rem" />
					</div>
				{/each}
			</div>
		</Card>
	{:else if peers.list.length === 0}
		<div class="empty-state">
			<svg width="48" height="48" viewBox="0 0 24 24" fill="none" stroke="var(--text-muted)" stroke-width="1.5">
				<circle cx="12" cy="12" r="10"/><line x1="2" y1="12" x2="22" y2="12"/><path d="M12 2a15.3 15.3 0 0 1 4 10 15.3 15.3 0 0 1-4 10 15.3 15.3 0 0 1-4-10 15.3 15.3 0 0 1 4-10z"/>
			</svg>
			<p class="empty-title">No peers connected</p>
			<p class="empty-desc">Your node has not established any peer connections yet. Peers will appear here once the node connects to the Bitcoin network.</p>
		</div>
	{:else}
		<!-- Summary -->
		<div class="stats-grid">
			<StatCard label="Total" value={String(peers.count)} sub="peers" />
			<StatCard label="Inbound" value={String(inboundCount)} sub="connections" />
			<StatCard label="Outbound" value={String(outboundCount)} sub="connections" />
			<StatCard label="V2 Transport" value={String(v2Count)} sub="BIP324 encrypted" />
		</div>

		<!-- Table -->
		<Card>
			<div class="table-scroll">
				<table class="peer-table">
					<thead>
						<tr>
							<th onclick={() => setSort('addr')}>Address{sortIndicator('addr')}</th>
							<th onclick={() => setSort('user_agent')}>User Agent{sortIndicator('user_agent')}</th>
							<th onclick={() => setSort('version')}>Version{sortIndicator('version')}</th>
							<th onclick={() => setSort('inbound')}>Direction{sortIndicator('inbound')}</th>
							<th onclick={() => setSort('v2_transport')}>Transport{sortIndicator('v2_transport')}</th>
							<th onclick={() => setSort('start_height')}>Start Height{sortIndicator('start_height')}</th>
						</tr>
					</thead>
					<tbody>
						{#each sorted as peer}
							<tr>
								<td class="addr">{peer.addr}</td>
								<td class="ua">{peer.user_agent}</td>
								<td>{peer.version}</td>
								<td>
									<Badge text={peer.inbound ? 'Inbound' : 'Outbound'} variant={peer.inbound ? 'accent' : 'default'} />
								</td>
								<td>
									{#if peer.v2_transport}
										<Badge text="V2" variant="success" />
									{:else}
										<Badge text="V1" variant="default" />
									{/if}
								</td>
								<td>{formatNumber(peer.start_height)}</td>
							</tr>
						{/each}
					</tbody>
				</table>
			</div>
		</Card>
	{/if}
</div>

<style>
	.content { padding: 2rem; max-width: 1200px; }
	.empty-state {
		display: flex; flex-direction: column; align-items: center;
		justify-content: center; min-height: 300px; gap: 0.75rem;
		color: var(--text-secondary);
	}
	.empty-title {
		font-family: var(--font-display); font-size: 1.3rem;
		color: var(--text-primary); font-weight: 400;
	}
	.empty-desc {
		font-family: var(--font-mono); font-size: 0.8rem;
		color: var(--text-muted); max-width: 400px; text-align: center; line-height: 1.6;
	}
	.skeleton-card {
		background: var(--bg-surface); border: 1px solid var(--border-dim);
		border-radius: 12px; padding: 1.25rem 1.5rem;
		display: flex; flex-direction: column; gap: 0.5rem;
	}
	.skeleton-table {
		display: flex; flex-direction: column; gap: 0.75rem;
		padding: 0.5rem 0;
	}
	.skeleton-row {
		display: flex; gap: 1rem; align-items: center;
		padding: 0.5rem 0; border-bottom: 1px solid var(--border-dim);
	}
	.skeleton-row:last-child { border-bottom: none; }
	.stats-grid {
		display: grid; grid-template-columns: repeat(auto-fill, minmax(180px, 1fr));
		gap: 1rem; margin-bottom: 1.5rem;
	}
	.table-scroll { overflow-x: auto; }
	.peer-table {
		width: 100%;
		border-collapse: collapse;
		font-family: var(--font-mono);
		font-size: 0.78rem;
	}
	.peer-table th {
		text-align: left;
		padding: 0.6rem 0.75rem;
		color: var(--text-muted);
		font-size: 0.65rem;
		text-transform: uppercase;
		letter-spacing: 0.08em;
		border-bottom: 1px solid var(--border-dim);
		cursor: pointer;
		user-select: none;
		white-space: nowrap;
	}
	.peer-table th:hover { color: var(--text-accent); }
	.peer-table td {
		padding: 0.6rem 0.75rem;
		border-bottom: 1px solid var(--border-dim);
		color: var(--text-secondary);
	}
	.peer-table tbody tr:last-child td { border-bottom: none; }
	.peer-table tbody tr:hover { background: var(--orange-glow); }
	.addr { color: var(--text-primary); white-space: nowrap; }
	.ua {
		max-width: 200px; overflow: hidden; text-overflow: ellipsis; white-space: nowrap;
	}
</style>
