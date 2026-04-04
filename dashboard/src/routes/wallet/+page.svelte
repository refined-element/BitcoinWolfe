<script lang="ts">
	import TopBar from '$lib/components/layout/TopBar.svelte';
	import StatCard from '$lib/components/shared/StatCard.svelte';
	import Card from '$lib/components/shared/Card.svelte';
	import Button from '$lib/components/shared/Button.svelte';
	import CopyButton from '$lib/components/shared/CopyButton.svelte';
	import Terminal from '$lib/components/shared/Terminal.svelte';
	import Modal from '$lib/components/shared/Modal.svelte';
	import Spinner from '$lib/components/shared/Spinner.svelte';
	import { nodeStore } from '$lib/stores/node.svelte';
	import { walletStore } from '$lib/stores/wallet.svelte';
	import { sidebarStore } from '$lib/stores/sidebar.svelte';
	import { createPoller } from '$lib/stores/polling.svelte';
	import { formatBtc, truncateMiddle } from '$lib/utils/format';
	import { generateQrDataUrl } from '$lib/utils/qr';
	import * as rpc from '$lib/api/rpc';
	import { onMount } from 'svelte';

	const node = nodeStore();
	const wallet = walletStore();
	const sidebar = sidebarStore();

	let mnemonic = $state('');
	let importPhrase = $state('');
	let importLoading = $state(false);
	let createLoading = $state(false);

	// Address generation
	let currentAddress = $state('');
	let addressQr = $state('');
	let addressLoading = $state(false);

	// Send flow
	let sendModalOpen = $state(false);
	let sendAddress = $state('');
	let sendAmount = $state('');
	let sendFeeRate = $state('');
	let sendLoading = $state(false);
	let sendPsbt = $state('');
	let sendStep = $state<'form' | 'sign' | 'done'>('form');
	let sendTxid = $state('');

	// Rescan
	let rescanLoading = $state(false);
	let rescanResult = $state('');

	onMount(() => {
		const poller = createPoller(() => wallet.refresh());
		return () => poller.stop();
	});

	async function handleCreate() {
		createLoading = true;
		try {
			const result = await rpc.createWallet();
			mnemonic = result.mnemonic;
			await wallet.refresh();
		} catch (e: any) {
			alert(e.message || 'Failed to create wallet');
		}
		createLoading = false;
	}

	async function handleImport() {
		importLoading = true;
		try {
			await rpc.importWallet(importPhrase.trim());
			importPhrase = '';
			await wallet.refresh();
		} catch (e: any) {
			alert(e.message || 'Failed to import wallet');
		}
		importLoading = false;
	}

	async function generateAddress() {
		addressLoading = true;
		try {
			currentAddress = await rpc.getNewAddress();
			addressQr = await generateQrDataUrl(`bitcoin:${currentAddress}`);
		} catch (e: any) {
			alert(e.message || 'Failed to generate address');
		}
		addressLoading = false;
	}

	async function handleSend() {
		sendLoading = true;
		try {
			const outputs: Record<string, number> = {};
			outputs[sendAddress] = parseFloat(sendAmount);
			const feeRate = sendFeeRate ? parseFloat(sendFeeRate) : undefined;
			const funded = await rpc.createFundedPsbt(outputs, feeRate);
			sendPsbt = funded.psbt;
			sendStep = 'sign';

			const processed = await rpc.processPsbt(funded.psbt);
			if (processed.complete) {
				const txid = await rpc.sendRawTransaction(processed.psbt);
				sendTxid = txid;
				sendStep = 'done';
				await wallet.refresh();
			}
		} catch (e: any) {
			alert(e.message || 'Send failed');
		}
		sendLoading = false;
	}

	function resetSend() {
		sendModalOpen = false;
		sendAddress = '';
		sendAmount = '';
		sendFeeRate = '';
		sendPsbt = '';
		sendStep = 'form';
		sendTxid = '';
	}

	async function handleRescan() {
		rescanLoading = true;
		rescanResult = '';
		try {
			const result = await rpc.rescanBlockchain();
			rescanResult = `Scanned ${result.blocks_scanned} blocks, found ${result.transactions_found} transactions`;
		} catch (e: any) {
			rescanResult = e.message || 'Rescan failed';
		}
		rescanLoading = false;
	}
</script>

<TopBar title="Wallet" info={node.info} onMenuToggle={() => sidebar.toggle()} />

<div class="content">
	{#if wallet.state === 'loading'}
		<div class="empty-state">
			<Spinner size={32} />
			<p>Loading wallet...</p>
		</div>

	{:else if wallet.state === 'no_wallet'}
		<!-- Wallet Setup -->
		{#if mnemonic}
			<Card>
				<Terminal title="Seed Phrase - SAVE THIS NOW">
					<span style="color: var(--color-warning); font-weight: 600;">WARNING: This will NOT be shown again!</span>

{mnemonic}
				</Terminal>
				<div style="margin-top: 1rem; display: flex; gap: 0.5rem; align-items: center;">
					<CopyButton text={mnemonic} />
					<span style="font-family: var(--font-mono); font-size: 0.75rem; color: var(--text-muted);">Copy seed phrase</span>
				</div>
				<Button variant="primary" onclick={() => { mnemonic = ''; wallet.refresh(); }} class="mt">
					I've saved my seed phrase
				</Button>
			</Card>
		{:else}
			<div class="setup-grid">
				<Card>
					<h2 class="card-title">Create New Wallet</h2>
					<p class="card-desc">Generate a new BIP39 wallet with a 12-word seed phrase.</p>
					<Button variant="primary" loading={createLoading} onclick={handleCreate}>
						Create Wallet
					</Button>
				</Card>

				<Card>
					<h2 class="card-title">Import Wallet</h2>
					<p class="card-desc">Restore from an existing 12-word BIP39 seed phrase.</p>
					<div class="form-group">
						<textarea
							class="input"
							rows="3"
							placeholder="word1 word2 word3 ... word12"
							bind:value={importPhrase}
						></textarea>
					</div>
					<Button
						variant="primary"
						loading={importLoading}
						disabled={importPhrase.trim().split(/\s+/).length < 12}
						onclick={handleImport}
					>
						Import Wallet
					</Button>
				</Card>
			</div>
		{/if}

	{:else if wallet.state === 'loaded' && wallet.info}
		<!-- Wallet Dashboard -->
		<div class="stats-grid">
			<StatCard label="Balance" value={formatBtc(wallet.info.balance)} sub="BTC confirmed" />
			<StatCard label="Unconfirmed" value={formatBtc(wallet.info.unconfirmed_balance)} sub="BTC pending" />
			<StatCard label="Immature" value={formatBtc(wallet.info.immature_balance)} sub="BTC immature" />
			<StatCard label="Transactions" value={String(wallet.info.txcount)} sub="total" />
		</div>

		<!-- Actions -->
		<div class="actions-row">
			<Button variant="primary" onclick={generateAddress} loading={addressLoading}>
				New Address
			</Button>
			<Button variant="secondary" onclick={() => sendModalOpen = true}>
				Send
			</Button>
			<Button variant="secondary" onclick={handleRescan} loading={rescanLoading}>
				Rescan
			</Button>
		</div>

		{#if rescanResult}
			<div class="rescan-result">{rescanResult}</div>
		{/if}

		<!-- Address + QR -->
		{#if currentAddress}
			<Card>
				<h3 class="card-subtitle">Receive Address</h3>
				<div class="address-display">
					{#if addressQr}
						<img src={addressQr} alt="QR Code" class="qr-code" />
					{/if}
					<div class="address-text">
						<code>{currentAddress}</code>
						<CopyButton text={currentAddress} />
					</div>
				</div>
			</Card>
		{/if}

		<!-- Transaction List -->
		{#if wallet.transactions.length > 0}
			<Card>
				<h3 class="card-subtitle">Recent Transactions</h3>
				<div class="tx-list">
					{#each wallet.transactions as tx}
						<div class="tx-item">
							<code class="tx-id">{truncateMiddle(tx.txid, 12, 12)}</code>
							<span class="tx-status" class:confirmed={tx.confirmed}>
								{tx.confirmed ? 'Confirmed' : 'Pending'}
							</span>
						</div>
					{/each}
				</div>
			</Card>
		{:else}
			<div class="empty-tx-state">
				<svg width="40" height="40" viewBox="0 0 24 24" fill="none" stroke="var(--text-muted)" stroke-width="1.5">
					<path d="M21 12V7H5a2 2 0 0 1 0-4h14v4"/><path d="M3 5v14a2 2 0 0 0 2 2h16v-5"/><circle cx="18" cy="16" r="1"/>
				</svg>
				<p class="empty-tx-title">No transactions yet</p>
				<p class="empty-tx-desc">Generate a receive address above to receive your first Bitcoin transaction.</p>
			</div>
		{/if}

		<!-- Send Modal -->
		<Modal open={sendModalOpen} title="Send Bitcoin" onclose={resetSend}>
			{#if sendStep === 'form'}
				<div class="form-group">
					<label class="form-label">Recipient Address</label>
					<input class="input" type="text" placeholder="bc1q..." bind:value={sendAddress} />
				</div>
				<div class="form-group">
					<label class="form-label">Amount (BTC)</label>
					<input class="input" type="number" step="0.00000001" placeholder="0.001" bind:value={sendAmount} />
				</div>
				<div class="form-group">
					<label class="form-label">Fee Rate (sat/vB, optional)</label>
					<input class="input" type="number" step="0.1" placeholder="auto" bind:value={sendFeeRate} />
				</div>
				<Button
					variant="primary"
					loading={sendLoading}
					disabled={!sendAddress || !sendAmount}
					onclick={handleSend}
				>
					Create & Sign Transaction
				</Button>
			{:else if sendStep === 'sign'}
				<p class="sign-msg">Signing transaction...</p>
				<Spinner />
			{:else if sendStep === 'done'}
				<div class="send-success">
					<p>Transaction broadcast!</p>
					<div class="tx-result">
						<code>{sendTxid}</code>
						<CopyButton text={sendTxid} />
					</div>
					<Button variant="secondary" onclick={resetSend}>Close</Button>
				</div>
			{/if}
		</Modal>

	{:else if wallet.state === 'error'}
		<div class="empty-state">
			<p class="error-text">{wallet.error}</p>
			<p class="hint">Check that the node is running and wallet is enabled in wolfe.toml</p>
		</div>
	{/if}
</div>

<style>
	.content {
		padding: 2rem;
		max-width: 1100px;
	}
	.empty-state {
		display: flex;
		flex-direction: column;
		align-items: center;
		justify-content: center;
		min-height: 300px;
		gap: 1rem;
		color: var(--text-secondary);
		font-family: var(--font-mono);
		font-size: 0.85rem;
	}
	.error-text { color: var(--color-error); }
	.setup-grid {
		display: grid;
		grid-template-columns: repeat(auto-fit, minmax(320px, 1fr));
		gap: 1.5rem;
	}
	.card-title {
		font-family: var(--font-display);
		font-size: 1.5rem;
		font-weight: 400;
		margin-bottom: 0.5rem;
	}
	.card-desc {
		color: var(--text-secondary);
		font-size: 0.9rem;
		margin-bottom: 1.25rem;
	}
	.card-subtitle {
		font-family: var(--font-mono);
		font-size: 0.7rem;
		color: var(--text-muted);
		text-transform: uppercase;
		letter-spacing: 0.08em;
		margin-bottom: 1rem;
	}
	.stats-grid {
		display: grid;
		grid-template-columns: repeat(auto-fill, minmax(200px, 1fr));
		gap: 1rem;
		margin-bottom: 1.5rem;
	}
	.actions-row {
		display: flex;
		gap: 0.75rem;
		margin-bottom: 1.5rem;
		flex-wrap: wrap;
	}
	.rescan-result {
		font-family: var(--font-mono);
		font-size: 0.8rem;
		color: var(--text-secondary);
		margin-bottom: 1rem;
		padding: 0.75rem 1rem;
		background: var(--bg-surface);
		border: 1px solid var(--border-dim);
		border-radius: 8px;
	}
	.address-display {
		display: flex;
		gap: 1.5rem;
		align-items: center;
		flex-wrap: wrap;
	}
	.qr-code {
		width: 160px;
		height: 160px;
		border-radius: 8px;
		border: 1px solid var(--border-dim);
	}
	.address-text {
		display: flex;
		align-items: center;
		gap: 0.5rem;
	}
	.address-text code {
		font-family: var(--font-mono);
		font-size: 0.85rem;
		color: var(--text-primary);
		word-break: break-all;
	}
	.tx-list {
		display: flex;
		flex-direction: column;
		gap: 0.5rem;
	}
	.tx-item {
		display: flex;
		justify-content: space-between;
		align-items: center;
		padding: 0.6rem 0;
		border-bottom: 1px solid var(--border-dim);
	}
	.tx-item:last-child { border-bottom: none; }
	.tx-id {
		font-family: var(--font-mono);
		font-size: 0.8rem;
		color: var(--text-secondary);
	}
	.tx-status {
		font-family: var(--font-mono);
		font-size: 0.7rem;
		color: var(--color-warning);
	}
	.tx-status.confirmed { color: var(--color-success); }

	.form-group {
		margin-bottom: 1rem;
	}
	.form-label {
		display: block;
		font-family: var(--font-mono);
		font-size: 0.7rem;
		color: var(--text-muted);
		text-transform: uppercase;
		letter-spacing: 0.06em;
		margin-bottom: 0.4rem;
	}
	.input {
		width: 100%;
		font-family: var(--font-mono);
		font-size: 0.85rem;
		padding: 0.65rem 0.85rem;
		background: var(--bg-raised);
		border: 1px solid var(--border-dim);
		border-radius: 8px;
		color: var(--text-primary);
		outline: none;
		transition: border-color 0.2s;
	}
	.input:focus { border-color: var(--border-accent); }
	textarea.input { resize: vertical; }

	.sign-msg {
		font-family: var(--font-mono);
		font-size: 0.85rem;
		color: var(--text-secondary);
		margin-bottom: 1rem;
	}
	.send-success {
		text-align: center;
	}
	.send-success p {
		font-family: var(--font-display);
		font-size: 1.3rem;
		color: var(--color-success);
		margin-bottom: 1rem;
	}
	.tx-result {
		display: flex;
		align-items: center;
		gap: 0.5rem;
		justify-content: center;
		margin-bottom: 1rem;
	}
	.tx-result code {
		font-family: var(--font-mono);
		font-size: 0.75rem;
		color: var(--text-secondary);
		word-break: break-all;
	}
	.empty-tx-state {
		display: flex;
		flex-direction: column;
		align-items: center;
		gap: 0.75rem;
		padding: 2.5rem 1rem;
		background: var(--bg-surface);
		border: 1px solid var(--border-dim);
		border-radius: 12px;
	}
	.empty-tx-title {
		font-family: var(--font-display);
		font-size: 1.2rem;
		color: var(--text-primary);
		font-weight: 400;
	}
	.empty-tx-desc {
		font-family: var(--font-mono);
		font-size: 0.8rem;
		color: var(--text-muted);
		max-width: 360px;
		text-align: center;
		line-height: 1.6;
	}
	:global(.mt) { margin-top: 1rem; }
</style>
