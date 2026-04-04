<script lang="ts">
	import TopBar from '$lib/components/layout/TopBar.svelte';
	import StatCard from '$lib/components/shared/StatCard.svelte';
	import Card from '$lib/components/shared/Card.svelte';
	import Button from '$lib/components/shared/Button.svelte';
	import Badge from '$lib/components/shared/Badge.svelte';
	import CopyButton from '$lib/components/shared/CopyButton.svelte';
	import Modal from '$lib/components/shared/Modal.svelte';
	import Skeleton from '$lib/components/shared/Skeleton.svelte';
	import ErrorCard from '$lib/components/shared/ErrorCard.svelte';
	import { nodeStore } from '$lib/stores/node.svelte';
	import { lightningStore } from '$lib/stores/lightning.svelte';
	import { sidebarStore } from '$lib/stores/sidebar.svelte';
	import { createPoller } from '$lib/stores/polling.svelte';
	import { formatNumber, formatSats, truncateMiddle } from '$lib/utils/format';
	import { generateQrDataUrl } from '$lib/utils/qr';
	import * as rpc from '$lib/api/rpc';
	import { onMount } from 'svelte';

	const node = nodeStore();
	const ln = lightningStore();
	const sidebar = sidebarStore();

	let totalCapacity = $derived(ln.channels.reduce((sum, ch) => sum + ch.capacity_sat, 0));

	// Connect peer
	let connectTarget = $state('');
	let connectLoading = $state(false);

	// Open channel
	let openModalOpen = $state(false);
	let openNodeId = $state('');
	let openAmount = $state('');
	let openLoading = $state(false);

	// Create invoice
	let invoiceModalOpen = $state(false);
	let invoiceAmount = $state('');
	let invoiceDesc = $state('');
	let invoiceLoading = $state(false);
	let invoiceResult = $state('');
	let invoiceQr = $state('');

	// Pay invoice
	let payModalOpen = $state(false);
	let payInvoice = $state('');
	let payLoading = $state(false);
	let payResult = $state('');

	onMount(() => {
		const poller = createPoller(() => ln.refresh());
		return () => poller.stop();
	});

	async function handleConnect() {
		connectLoading = true;
		try {
			await rpc.lnConnect(connectTarget.trim());
			connectTarget = '';
			await ln.refresh();
		} catch (e: any) { alert(e.message); }
		connectLoading = false;
	}

	async function handleOpenChannel() {
		openLoading = true;
		try {
			await rpc.lnOpenChannel(openNodeId.trim(), parseInt(openAmount));
			openModalOpen = false;
			openNodeId = '';
			openAmount = '';
			await ln.refresh();
		} catch (e: any) { alert(e.message); }
		openLoading = false;
	}

	async function handleCloseChannel(channelId: string, counterparty: string) {
		if (!confirm('Close this channel cooperatively?')) return;
		try {
			await rpc.lnCloseChannel(channelId, counterparty);
			await ln.refresh();
		} catch (e: any) { alert(e.message); }
	}

	async function handleForceCloseChannel(channelId: string, counterparty: string) {
		if (!confirm('Force close this channel? This will broadcast the latest commitment transaction on-chain. Your funds may be locked for a timelock period. Are you sure?')) return;
		try {
			await rpc.lnCloseChannel(channelId, counterparty, true);
			await ln.refresh();
		} catch (e: any) { alert(e.message); }
	}

	async function handleCreateInvoice() {
		invoiceLoading = true;
		invoiceResult = '';
		invoiceQr = '';
		try {
			const amt = invoiceAmount ? parseInt(invoiceAmount) : undefined;
			const result = await rpc.lnCreateInvoice(amt, invoiceDesc || undefined);
			invoiceResult = result.invoice;
			invoiceQr = await generateQrDataUrl(result.invoice);
		} catch (e: any) { alert(e.message); }
		invoiceLoading = false;
	}

	async function handlePay() {
		payLoading = true;
		payResult = '';
		try {
			const result = await rpc.lnPay(payInvoice.trim());
			payResult = `Payment sent! ID: ${result.payment_id}`;
			payInvoice = '';
		} catch (e: any) { alert(e.message); }
		payLoading = false;
	}
</script>

<TopBar title="Lightning" info={node.info} onMenuToggle={() => sidebar.toggle()} />

<div class="content">
	{#if ln.error}
		<ErrorCard message={ln.error} onretry={() => ln.refresh()} />
		<p class="hint">Lightning may not be enabled in wolfe.toml</p>
	{:else if !ln.info}
		<!-- Skeleton loading state -->
		<div class="stats-grid">
			{#each Array(3) as _}
				<div class="skeleton-card">
					<Skeleton width="60%" height="0.7rem" />
					<Skeleton width="40%" height="1.75rem" />
					<Skeleton width="80%" height="0.75rem" />
				</div>
			{/each}
		</div>
		<div class="skeleton-card" style="margin-bottom: 1rem;">
			<Skeleton width="30%" height="0.7rem" />
			<Skeleton width="100%" height="0.85rem" />
		</div>
	{:else}
		<!-- Stats -->
		<div class="stats-grid">
			<StatCard label="Channels" value={String(ln.info.num_channels)} sub="{ln.info.num_active_channels} active" />
			<StatCard label="Peers" value={String(ln.info.num_peers)} sub="connected" />
			<StatCard label="Total Capacity" value={formatSats(totalCapacity)} sub="sats" />
		</div>

		<!-- Node ID -->
		<Card>
			<h3 class="card-subtitle">Node ID</h3>
			<div class="id-row">
				<code class="node-id">{ln.info.node_id}</code>
				<CopyButton text={ln.info.node_id} />
			</div>
		</Card>

		<!-- Actions -->
		<div class="actions-row">
			<Button variant="primary" onclick={() => openModalOpen = true}>Open Channel</Button>
			<Button variant="secondary" onclick={() => invoiceModalOpen = true}>Create Invoice</Button>
			<Button variant="secondary" onclick={() => payModalOpen = true}>Pay Invoice</Button>
		</div>

		<!-- Connect Peer -->
		<Card>
			<h3 class="card-subtitle">Connect Peer</h3>
			<div class="connect-row">
				<input class="input" placeholder="pubkey@host:port" bind:value={connectTarget} />
				<Button variant="primary" loading={connectLoading} disabled={!connectTarget.trim()} onclick={handleConnect}>
					Connect
				</Button>
			</div>
		</Card>

		<!-- Channels -->
		{#if ln.channels.length > 0}
			<Card>
				<h3 class="card-subtitle">Channels</h3>
				<div class="channel-list">
					{#each ln.channels as ch}
						{@const total = ch.outbound_capacity_msat + ch.inbound_capacity_msat}
						{@const outPct = total > 0 ? (ch.outbound_capacity_msat / total) * 100 : 50}
						<div class="channel-item">
							<div class="channel-header">
								<div class="channel-id-info">
									<code class="channel-peer">{truncateMiddle(ch.counterparty, 10, 10)}</code>
									{#if ch.short_channel_id}
										<span class="short-channel-id">{ch.short_channel_id}</span>
									{/if}
								</div>
								<div class="channel-badges">
									{#if ch.is_usable}
										<Badge text="Active" variant="success" />
									{:else if ch.is_channel_ready}
										<Badge text="Ready" variant="warning" />
									{:else}
										<Badge text="Pending" variant="default" />
									{/if}
									<Badge text={ch.is_outbound ? 'Outbound' : 'Inbound'} variant="default" />
								</div>
							</div>
							<div class="capacity-bar-container">
								<div class="capacity-labels">
									<span>{formatNumber(Math.floor(ch.outbound_capacity_msat / 1000))} sat out</span>
									<span>{formatNumber(ch.capacity_sat)} sat</span>
									<span>{formatNumber(Math.floor(ch.inbound_capacity_msat / 1000))} sat in</span>
								</div>
								<div class="capacity-bar">
									<div class="capacity-out" style="width: {outPct}%"></div>
								</div>
							</div>
							<div class="channel-actions">
								<button class="close-link" onclick={() => handleCloseChannel(ch.channel_id, ch.counterparty)}>
									Close Channel
								</button>
								<button class="force-close-link" onclick={() => handleForceCloseChannel(ch.channel_id, ch.counterparty)}>
									Force Close
								</button>
							</div>
						</div>
					{/each}
				</div>
			</Card>
		{:else}
			<div class="empty-state">
				<svg width="48" height="48" viewBox="0 0 24 24" fill="none" stroke="var(--text-muted)" stroke-width="1.5">
					<polygon points="13 2 3 14 12 14 11 22 21 10 12 10 13 2"/>
				</svg>
				<p class="empty-title">No channels yet</p>
				<p class="empty-desc">Open a Lightning channel to start sending and receiving payments instantly with low fees.</p>
				<Button variant="primary" onclick={() => openModalOpen = true}>Open Channel</Button>
			</div>
		{/if}

		<!-- Payment History -->
		{#if ln.payments.length > 0}
			<Card>
				<h3 class="card-subtitle">Payment History</h3>
				<div class="peer-table">
					<div class="peer-header">
						<span>Time</span>
						<span>Direction</span>
						<span>Amount</span>
						<span>Status</span>
					</div>
					{#each ln.payments as payment}
						<div class="peer-row">
							<span>{new Date(payment.timestamp * 1000).toLocaleString()}</span>
							<Badge text={payment.direction === 'receive' ? 'Received' : 'Sent'} variant={payment.direction === 'receive' ? 'success' : 'default'} />
							<span>{payment.amount_msat != null ? formatNumber(Math.floor(payment.amount_msat / 1000)) + ' sat' : '—'}</span>
							<Badge text={payment.status} variant={payment.status === 'completed' ? 'success' : 'danger'} />
						</div>
					{/each}
				</div>
			</Card>
		{/if}

		<!-- Peers Table -->
		{#if ln.peers.length > 0}
			<Card>
				<h3 class="card-subtitle">Lightning Peers</h3>
				<div class="peer-table">
					<div class="peer-header">
						<span>Node ID</span>
						<span>Address</span>
						<span>Direction</span>
					</div>
					{#each ln.peers as peer}
						<div class="peer-row">
							<code>{truncateMiddle(peer.node_id, 10, 10)}</code>
							<span>{peer.address || '—'}</span>
							<Badge text={peer.inbound ? 'Inbound' : 'Outbound'} variant="default" />
						</div>
					{/each}
				</div>
			</Card>
		{/if}

		<!-- Open Channel Modal -->
		<Modal open={openModalOpen} title="Open Channel" onclose={() => openModalOpen = false}>
			<div class="form-group">
				<label class="form-label">Node ID</label>
				<input class="input" placeholder="02abc..." bind:value={openNodeId} />
			</div>
			<div class="form-group">
				<label class="form-label">Amount (sats)</label>
				<input class="input" type="number" placeholder="100000" bind:value={openAmount} />
			</div>
			<Button variant="primary" loading={openLoading} disabled={!openNodeId || !openAmount} onclick={handleOpenChannel}>
				Open Channel
			</Button>
		</Modal>

		<!-- Create Invoice Modal -->
		<Modal open={invoiceModalOpen} title="Create Invoice" onclose={() => { invoiceModalOpen = false; invoiceResult = ''; invoiceQr = ''; }}>
			{#if !invoiceResult}
				<div class="form-group">
					<label class="form-label">Amount (msat, optional for any-amount)</label>
					<input class="input" type="number" placeholder="50000" bind:value={invoiceAmount} />
				</div>
				<div class="form-group">
					<label class="form-label">Description (optional)</label>
					<input class="input" placeholder="Payment for..." bind:value={invoiceDesc} />
				</div>
				<Button variant="primary" loading={invoiceLoading} onclick={handleCreateInvoice}>
					Generate Invoice
				</Button>
			{:else}
				<div class="invoice-result">
					{#if invoiceQr}
						<img src={invoiceQr} alt="Invoice QR" class="invoice-qr" />
					{/if}
					<div class="invoice-text">
						<code>{invoiceResult}</code>
						<CopyButton text={invoiceResult} />
					</div>
				</div>
			{/if}
		</Modal>

		<!-- Pay Invoice Modal -->
		<Modal open={payModalOpen} title="Pay Invoice" onclose={() => { payModalOpen = false; payResult = ''; }}>
			{#if !payResult}
				<div class="form-group">
					<label class="form-label">BOLT11 Invoice</label>
					<textarea class="input" rows="4" placeholder="lnbc..." bind:value={payInvoice}></textarea>
				</div>
				<Button variant="primary" loading={payLoading} disabled={!payInvoice.trim()} onclick={handlePay}>
					Pay
				</Button>
			{:else}
				<p class="pay-success">{payResult}</p>
				<Button variant="secondary" onclick={() => { payModalOpen = false; payResult = ''; }}>Close</Button>
			{/if}
		</Modal>
	{/if}
</div>

<style>
	.content { padding: 2rem; max-width: 1100px; }
	.empty-state {
		display: flex; flex-direction: column; align-items: center;
		justify-content: center; min-height: 200px; gap: 0.75rem;
		color: var(--text-secondary); padding: 2rem 0;
	}
	.empty-title {
		font-family: var(--font-display); font-size: 1.3rem;
		color: var(--text-primary); font-weight: 400;
	}
	.empty-desc {
		font-family: var(--font-mono); font-size: 0.8rem;
		color: var(--text-muted); max-width: 400px; text-align: center; line-height: 1.6;
		margin-bottom: 0.5rem;
	}
	.hint {
		color: var(--text-muted); font-family: var(--font-mono);
		font-size: 0.8rem; text-align: center; margin-top: 1rem;
	}
	.skeleton-card {
		background: var(--bg-surface); border: 1px solid var(--border-dim);
		border-radius: 12px; padding: 1.25rem 1.5rem;
		display: flex; flex-direction: column; gap: 0.5rem;
	}
	.stats-grid { display: grid; grid-template-columns: repeat(auto-fill, minmax(200px, 1fr)); gap: 1rem; margin-bottom: 1.5rem; }
	.card-subtitle {
		font-family: var(--font-mono); font-size: 0.7rem; color: var(--text-muted);
		text-transform: uppercase; letter-spacing: 0.08em; margin-bottom: 1rem;
	}
	.id-row { display: flex; align-items: center; gap: 0.5rem; }
	.node-id { font-family: var(--font-mono); font-size: 0.78rem; color: var(--text-secondary); word-break: break-all; }
	.actions-row { display: flex; gap: 0.75rem; margin: 1.5rem 0; flex-wrap: wrap; }
	.connect-row { display: flex; gap: 0.75rem; align-items: center; }
	.connect-row .input { flex: 1; }

	.channel-list { display: flex; flex-direction: column; gap: 1rem; }
	.channel-item {
		padding: 1rem; background: var(--bg-raised); border-radius: 8px;
		border: 1px solid var(--border-dim);
	}
	.channel-header { display: flex; justify-content: space-between; align-items: center; margin-bottom: 0.75rem; flex-wrap: wrap; gap: 0.5rem; }
	.channel-id-info { display: flex; flex-direction: column; gap: 0.2rem; }
	.channel-peer { font-family: var(--font-mono); font-size: 0.8rem; color: var(--text-primary); }
	.short-channel-id { font-family: var(--font-mono); font-size: 0.7rem; color: var(--text-muted); }
	.channel-badges { display: flex; gap: 0.35rem; }
	.capacity-bar-container { margin-bottom: 0.5rem; }
	.capacity-labels {
		display: flex; justify-content: space-between; font-family: var(--font-mono);
		font-size: 0.65rem; color: var(--text-muted); margin-bottom: 0.35rem;
	}
	.capacity-bar { height: 8px; background: var(--bg-surface); border-radius: 4px; overflow: hidden; }
	.capacity-out { height: 100%; background: linear-gradient(90deg, var(--text-accent), var(--text-accent-bright)); border-radius: 4px; transition: width 0.3s; }
	.channel-actions { display: flex; justify-content: flex-end; gap: 1rem; }
	.close-link {
		background: none; border: none; color: var(--text-muted); cursor: pointer;
		font-family: var(--font-mono); font-size: 0.7rem; transition: color 0.2s;
	}
	.close-link:hover { color: var(--color-error); }
	.force-close-link {
		background: none; border: none; color: var(--text-muted); cursor: pointer;
		font-family: var(--font-mono); font-size: 0.7rem; transition: color 0.2s;
	}
	.force-close-link:hover { color: var(--color-error); font-weight: 600; }

	.peer-table { font-family: var(--font-mono); font-size: 0.78rem; }
	.peer-header {
		display: grid; grid-template-columns: 1fr 1fr auto; gap: 1rem; padding: 0.5rem 0;
		color: var(--text-muted); border-bottom: 1px solid var(--border-dim);
		font-size: 0.7rem; text-transform: uppercase; letter-spacing: 0.06em;
	}
	.peer-row {
		display: grid; grid-template-columns: 1fr 1fr auto; gap: 1rem; padding: 0.6rem 0;
		border-bottom: 1px solid var(--border-dim); align-items: center; color: var(--text-secondary);
	}
	.peer-row:last-child { border-bottom: none; }

	.form-group { margin-bottom: 1rem; }
	.form-label {
		display: block; font-family: var(--font-mono); font-size: 0.7rem;
		color: var(--text-muted); text-transform: uppercase; letter-spacing: 0.06em; margin-bottom: 0.4rem;
	}
	.input {
		width: 100%; font-family: var(--font-mono); font-size: 0.85rem; padding: 0.65rem 0.85rem;
		background: var(--bg-raised); border: 1px solid var(--border-dim); border-radius: 8px;
		color: var(--text-primary); outline: none; transition: border-color 0.2s;
	}
	.input:focus { border-color: var(--border-accent); }
	textarea.input { resize: vertical; }

	.invoice-result { text-align: center; }
	.invoice-qr { width: 200px; height: 200px; border-radius: 8px; border: 1px solid var(--border-dim); margin-bottom: 1rem; }
	.invoice-text { display: flex; align-items: flex-start; gap: 0.5rem; justify-content: center; }
	.invoice-text code { font-family: var(--font-mono); font-size: 0.7rem; color: var(--text-secondary); word-break: break-all; max-width: 350px; }
	.pay-success { font-family: var(--font-mono); font-size: 0.85rem; color: var(--color-success); margin-bottom: 1rem; }
</style>
