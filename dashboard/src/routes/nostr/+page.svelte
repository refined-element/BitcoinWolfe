<script lang="ts">
	import TopBar from '$lib/components/layout/TopBar.svelte';
	import Card from '$lib/components/shared/Card.svelte';
	import Button from '$lib/components/shared/Button.svelte';
	import Badge from '$lib/components/shared/Badge.svelte';
	import CopyButton from '$lib/components/shared/CopyButton.svelte';
	import Spinner from '$lib/components/shared/Spinner.svelte';
	import { nodeStore } from '$lib/stores/node.svelte';
	import { nostrStore } from '$lib/stores/nostr.svelte';
	import { sidebarStore } from '$lib/stores/sidebar.svelte';
	import { createPoller } from '$lib/stores/polling.svelte';
	import * as rpc from '$lib/api/rpc';
	import { onMount } from 'svelte';

	const node = nodeStore();
	const nostr = nostrStore();
	const sidebar = sidebarStore();

	let newRelay = $state('');
	let addRelayLoading = $state(false);
	let publishContent = $state('');
	let publishKind = $state('1');
	let publishLoading = $state(false);
	let publishResult = $state('');

	onMount(() => {
		const poller = createPoller(() => nostr.refresh());
		return () => poller.stop();
	});

	async function addRelay() {
		addRelayLoading = true;
		try {
			await rpc.nostrAddRelay(newRelay.trim());
			newRelay = '';
			await nostr.refresh();
		} catch (e: any) { alert(e.message); }
		addRelayLoading = false;
	}

	async function removeRelay(url: string) {
		try {
			await rpc.nostrRemoveRelay(url);
			await nostr.refresh();
		} catch (e: any) { alert(e.message); }
	}

	async function publish() {
		publishLoading = true;
		publishResult = '';
		try {
			const result = await rpc.nostrPublish(publishContent, parseInt(publishKind));
			publishResult = `Published event ${result.event_id} (kind ${result.kind})`;
			publishContent = '';
		} catch (e: any) { alert(e.message); }
		publishLoading = false;
	}
</script>

<TopBar title="Nostr" info={node.info} onMenuToggle={() => sidebar.toggle()} />

<div class="content">
	{#if nostr.error}
		<div class="empty-state">
			<p class="error-text">{nostr.error}</p>
			<p class="hint">Nostr may not be enabled in wolfe.toml</p>
		</div>
	{:else if !nostr.info}
		<div class="empty-state"><Spinner size={32} /><p>Loading Nostr...</p></div>
	{:else}
		<!-- npub -->
		<Card>
			<h3 class="card-subtitle">Public Key (npub)</h3>
			<div class="npub-row">
				<code class="npub">{nostr.info.npub}</code>
				<CopyButton text={nostr.info.npub} />
			</div>
			<div class="status-row">
				<Badge text={nostr.info.enabled ? 'Enabled' : 'Disabled'} variant={nostr.info.enabled ? 'success' : 'error'} />
				<span class="relay-count">{nostr.info.relay_count} relay{nostr.info.relay_count !== 1 ? 's' : ''}</span>
			</div>
		</Card>

		<!-- Relays -->
		<Card>
			<h3 class="card-subtitle">Relays</h3>
			<div class="relay-list">
				{#each nostr.info.relays as relay}
					<div class="relay-item">
						<div class="relay-info">
							<span class="relay-status-dot"></span>
							<span class="relay-url">{relay}</span>
						</div>
						<button class="remove-btn" onclick={() => removeRelay(relay)}>Remove</button>
					</div>
				{:else}
					<p class="no-relays">No relays configured</p>
				{/each}
			</div>
			<div class="add-relay-row">
				<input class="input" placeholder="wss://relay.example.com" bind:value={newRelay} />
				<Button variant="primary" loading={addRelayLoading} disabled={!newRelay.trim()} onclick={addRelay}>
					Add
				</Button>
			</div>
		</Card>

		<!-- Publish -->
		<Card>
			<h3 class="card-subtitle">Publish Event</h3>
			<div class="form-group">
				<label class="form-label">Content</label>
				<textarea class="input" rows="4" placeholder="Your note content..." bind:value={publishContent}></textarea>
			</div>
			<div class="form-group">
				<label class="form-label">Kind</label>
				<input class="input" type="number" bind:value={publishKind} />
			</div>
			{#if publishResult}
				<div class="publish-result">{publishResult}</div>
			{/if}
			<Button variant="primary" loading={publishLoading} disabled={!publishContent.trim()} onclick={publish}>
				Publish
			</Button>
		</Card>
	{/if}
</div>

<style>
	.content { padding: 2rem; max-width: 900px; }
	.empty-state {
		display: flex; flex-direction: column; align-items: center;
		justify-content: center; min-height: 300px; gap: 1rem;
		color: var(--text-secondary); font-family: var(--font-mono); font-size: 0.85rem;
	}
	.error-text { color: var(--color-error); }
	.hint { color: var(--text-muted); font-size: 0.8rem; }
	.card-subtitle {
		font-family: var(--font-mono); font-size: 0.7rem; color: var(--text-muted);
		text-transform: uppercase; letter-spacing: 0.08em; margin-bottom: 1rem;
	}
	.npub-row { display: flex; align-items: center; gap: 0.5rem; margin-bottom: 0.75rem; }
	.npub { font-family: var(--font-mono); font-size: 0.8rem; color: var(--text-primary); word-break: break-all; }
	.status-row { display: flex; align-items: center; gap: 0.75rem; }
	.relay-count { font-family: var(--font-mono); font-size: 0.75rem; color: var(--text-muted); }

	.relay-list { margin-bottom: 1rem; }
	.relay-item {
		display: flex; justify-content: space-between; align-items: center;
		padding: 0.6rem 0; border-bottom: 1px solid var(--border-dim);
	}
	.relay-item:last-child { border-bottom: none; }
	.relay-info { display: flex; align-items: center; gap: 0.5rem; }
	.relay-status-dot {
		width: 8px; height: 8px; border-radius: 50%;
		background: var(--color-success);
		box-shadow: 0 0 6px rgba(52, 211, 153, 0.4);
		flex-shrink: 0;
	}
	.relay-url { font-family: var(--font-mono); font-size: 0.8rem; color: var(--text-secondary); }
	.remove-btn {
		background: none; border: none; color: var(--text-muted); cursor: pointer;
		font-family: var(--font-mono); font-size: 0.7rem; transition: color 0.2s;
	}
	.remove-btn:hover { color: var(--color-error); }
	.no-relays { font-family: var(--font-mono); font-size: 0.8rem; color: var(--text-muted); }
	.add-relay-row { display: flex; gap: 0.75rem; align-items: center; }
	.add-relay-row .input { flex: 1; }

	.form-group { margin-bottom: 1rem; }
	.form-label {
		display: block; font-family: var(--font-mono); font-size: 0.7rem; color: var(--text-muted);
		text-transform: uppercase; letter-spacing: 0.06em; margin-bottom: 0.4rem;
	}
	.input {
		width: 100%; font-family: var(--font-mono); font-size: 0.85rem; padding: 0.65rem 0.85rem;
		background: var(--bg-raised); border: 1px solid var(--border-dim); border-radius: 8px;
		color: var(--text-primary); outline: none; transition: border-color 0.2s;
	}
	.input:focus { border-color: var(--border-accent); }
	textarea.input { resize: vertical; }
	.publish-result {
		font-family: var(--font-mono); font-size: 0.8rem; color: var(--color-success);
		margin-bottom: 0.75rem; padding: 0.5rem 0.75rem;
		background: rgba(52, 211, 153, 0.06); border-radius: 6px;
	}
</style>
