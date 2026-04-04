<script lang="ts">
	import TopBar from '$lib/components/layout/TopBar.svelte';
	import Card from '$lib/components/shared/Card.svelte';
	import Button from '$lib/components/shared/Button.svelte';
	import Badge from '$lib/components/shared/Badge.svelte';
	import Modal from '$lib/components/shared/Modal.svelte';
	import { nodeStore } from '$lib/stores/node.svelte';
	import { connectionStore } from '$lib/stores/connection.svelte';
	import { sidebarStore } from '$lib/stores/sidebar.svelte';
	import { fetchRest } from '$lib/api/client';
	import * as rpc from '$lib/api/rpc';

	const node = nodeStore();
	const conn = connectionStore();
	const sidebar = sidebarStore();

	let url = $state(conn.current.url);
	let user = $state(conn.current.user);
	let password = $state(conn.current.password);
	let pollInterval = $state(conn.current.pollInterval);
	let testStatus = $state<'idle' | 'testing' | 'success' | 'error'>('idle');
	let testError = $state('');
	let stopModalOpen = $state(false);
	let stopLoading = $state(false);

	function saveSettings() {
		conn.set({ url, user, password, pollInterval });
		node.refresh();
	}

	async function testConnection() {
		testStatus = 'testing';
		testError = '';
		// Temporarily apply to test
		conn.set({ url, user, password, pollInterval });
		try {
			await fetchRest('/api/info');
			testStatus = 'success';
		} catch (e: any) {
			testStatus = 'error';
			testError = e.message || 'Connection failed';
		}
	}

	async function handleStop() {
		stopLoading = true;
		try {
			await rpc.stopNode();
		} catch { /* node shuts down, connection drops */ }
		stopLoading = false;
		stopModalOpen = false;
	}
</script>

<TopBar title="Settings" info={node.info} onMenuToggle={() => sidebar.toggle()} />

<div class="content">
	<!-- Connection -->
	<Card>
		<h2 class="card-title">Node Connection</h2>
		<p class="card-desc">Configure the URL and credentials for your BitcoinWolfe node.</p>

		<div class="form-group">
			<label class="form-label">Node URL</label>
			<input class="input" type="text" placeholder="http://127.0.0.1:8332" bind:value={url} />
			<span class="form-hint">Leave blank in dev mode (proxied via Vite). Set for static build.</span>
		</div>
		<div class="form-row">
			<div class="form-group">
				<label class="form-label">Username</label>
				<input class="input" type="text" placeholder="bitcoin" bind:value={user} />
			</div>
			<div class="form-group">
				<label class="form-label">Password</label>
				<input class="input" type="password" placeholder="password" bind:value={password} />
			</div>
		</div>
		<div class="form-group">
			<label class="form-label">Poll Interval (ms)</label>
			<div class="slider-row">
				<input type="range" min="1000" max="30000" step="1000" bind:value={pollInterval} class="slider" />
				<span class="slider-value">{pollInterval / 1000}s</span>
			</div>
		</div>

		<div class="button-row">
			<Button variant="secondary" onclick={testConnection} loading={testStatus === 'testing'}>
				Test Connection
			</Button>
			<Button variant="primary" onclick={saveSettings}>
				Save
			</Button>
			{#if testStatus === 'success'}
				<Badge text="Connected" variant="success" />
			{:else if testStatus === 'error'}
				<Badge text={testError} variant="error" />
			{/if}
		</div>
	</Card>

	<!-- CORS Note -->
	<Card>
		<h2 class="card-title">Static Build Setup</h2>
		<p class="card-desc">When running the dashboard as a static build, the browser connects directly to your node's REST API. Follow these steps to configure CORS:</p>

		<ol class="setup-steps">
			<li>Open your <code class="inline-code">wolfe.toml</code> configuration file</li>
			<li>Add the dashboard's URL to the <code class="inline-code">cors_origins</code> array in the <code class="inline-code">[rpc]</code> section</li>
			<li>Restart the node for changes to take effect</li>
			<li>In the dashboard's <strong>Node Connection</strong> settings above, set the Node URL to your node's address (e.g. <code class="inline-code">http://127.0.0.1:8332</code>)</li>
		</ol>

		<div class="code-block">
			<code><span class="comment"># wolfe.toml</span></code><br/>
			<code><span class="key">[rpc]</span></code><br/>
			<code><span class="key">bind</span> = <span class="str">"0.0.0.0:8332"</span></code><br/>
			<code><span class="key">user</span> = <span class="str">"bitcoin"</span></code><br/>
			<code><span class="key">password</span> = <span class="str">"password"</span></code><br/>
			<code><span class="key">cors_origins</span> = [<span class="str">"http://localhost:3000"</span>, <span class="str">"http://your-dashboard-host"</span>]</code>
		</div>

		<p class="setup-note">Replace <code class="inline-code">http://your-dashboard-host</code> with the actual URL where you serve the static dashboard build.</p>
	</Card>

	<!-- About -->
	<Card>
		<h2 class="card-title">About</h2>
		<p class="version-label">Dashboard <span class="version-value">v0.1.0</span></p>
		{#if node.info}
			<p class="version-label">Node <span class="version-value">{node.info.version}</span></p>
			<p class="version-label">User Agent <span class="version-value">{node.info.user_agent}</span></p>
		{/if}
	</Card>

	<!-- Stop Node -->
	<Card>
		<h2 class="card-title">Danger Zone</h2>
		<Button variant="danger" onclick={() => stopModalOpen = true}>
			Stop Node
		</Button>
	</Card>

	<Modal open={stopModalOpen} title="Stop Node" onclose={() => stopModalOpen = false}>
		<p class="stop-warning">Are you sure you want to stop the BitcoinWolfe node? This will terminate all connections and services.</p>
		<div class="button-row">
			<Button variant="secondary" onclick={() => stopModalOpen = false}>Cancel</Button>
			<Button variant="danger" loading={stopLoading} onclick={handleStop}>Yes, Stop Node</Button>
		</div>
	</Modal>
</div>

<style>
	.content { padding: 2rem; max-width: 700px; }
	.card-title {
		font-family: var(--font-display); font-size: 1.4rem;
		font-weight: 400; margin-bottom: 0.5rem;
	}
	.card-desc { color: var(--text-secondary); font-size: 0.9rem; margin-bottom: 1.5rem; }
	.form-group { margin-bottom: 1rem; }
	.form-label {
		display: block; font-family: var(--font-mono); font-size: 0.7rem; color: var(--text-muted);
		text-transform: uppercase; letter-spacing: 0.06em; margin-bottom: 0.4rem;
	}
	.form-hint {
		display: block; font-family: var(--font-mono); font-size: 0.65rem;
		color: var(--text-muted); margin-top: 0.3rem;
	}
	.form-row {
		display: grid; grid-template-columns: 1fr 1fr; gap: 1rem;
	}
	.input {
		width: 100%; font-family: var(--font-mono); font-size: 0.85rem; padding: 0.65rem 0.85rem;
		background: var(--bg-raised); border: 1px solid var(--border-dim); border-radius: 8px;
		color: var(--text-primary); outline: none; transition: border-color 0.2s;
	}
	.input:focus { border-color: var(--border-accent); }
	.slider-row { display: flex; align-items: center; gap: 1rem; }
	.slider {
		flex: 1; appearance: none; height: 4px; background: var(--bg-raised);
		border-radius: 2px; outline: none;
	}
	.slider::-webkit-slider-thumb {
		appearance: none; width: 16px; height: 16px; border-radius: 50%;
		background: var(--text-accent); cursor: pointer;
	}
	.slider-value {
		font-family: var(--font-mono); font-size: 0.8rem; color: var(--text-accent);
		min-width: 3ch;
	}
	.button-row { display: flex; gap: 0.75rem; align-items: center; flex-wrap: wrap; }
	.code-block {
		font-family: var(--font-mono); font-size: 0.8rem; padding: 1rem;
		background: var(--bg-raised); border-radius: 8px; border: 1px solid var(--border-dim);
		color: var(--text-secondary); line-height: 1.7;
	}
	.code-block .key { color: var(--text-accent); }
	.code-block .str { color: #7ec699; }
	.code-block .comment { color: var(--text-muted); font-style: italic; }
	.setup-steps {
		list-style: decimal; padding-left: 1.25rem; margin-bottom: 1.25rem;
		color: var(--text-secondary); font-size: 0.88rem; line-height: 1.8;
	}
	.setup-steps li { padding-left: 0.25rem; }
	.inline-code {
		font-family: var(--font-mono); font-size: 0.8rem; color: var(--text-accent);
		background: var(--bg-raised); padding: 0.1rem 0.35rem; border-radius: 4px;
	}
	.setup-note {
		color: var(--text-muted); font-size: 0.8rem; margin-top: 0.75rem;
		font-family: var(--font-mono);
	}
	.version-label {
		font-family: var(--font-mono); font-size: 0.8rem; color: var(--text-muted);
		margin-bottom: 0.35rem;
	}
	.version-value { color: var(--text-secondary); }
	.stop-warning { color: var(--text-secondary); margin-bottom: 1.5rem; font-size: 0.9rem; }
</style>
