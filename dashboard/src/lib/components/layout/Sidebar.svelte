<script lang="ts">
	import { page } from '$app/state';

	let { collapsed = false, connected = false, mobileOpen = false, onToggle, onMobileClose }: {
		collapsed?: boolean;
		connected?: boolean;
		mobileOpen?: boolean;
		onToggle: () => void;
		onMobileClose?: () => void;
	} = $props();

	const nav = [
		{ href: '/', label: 'Overview', icon: 'grid' },
		{ href: '/wallet', label: 'Wallet', icon: 'wallet' },
		{ href: '/lightning', label: 'Lightning', icon: 'zap' },
		{ href: '/nostr', label: 'Nostr', icon: 'radio' },
		{ href: '/peers', label: 'Peers', icon: 'globe' },
		{ href: '/settings', label: 'Settings', icon: 'settings' },
	];

	function isActive(href: string): boolean {
		const pathname = page.url?.pathname ?? '/';
		if (href === '/') return pathname === '/';
		return pathname.startsWith(href);
	}
</script>

<!-- svelte-ignore a11y_no_static_element_interactions -->
{#if mobileOpen}
	<!-- svelte-ignore a11y_click_events_have_key_events -->
	<div class="mobile-backdrop" onclick={onMobileClose}></div>
{/if}

<aside class="sidebar" class:collapsed class:mobile-open={mobileOpen}>
	<div class="sidebar-header">
		<a href="/" class="logo" onclick={onMobileClose}>
			<img src="/wolf-icon.png" alt="BitcoinWolfe" width="28" height="28" class="wolf-icon" />
			{#if !collapsed || mobileOpen}
				<span class="logo-text">BitcoinWolfe</span>
			{/if}
		</a>
		<button class="collapse-btn" onclick={onToggle} title={collapsed ? 'Expand sidebar' : 'Collapse sidebar'}>
			<svg width="16" height="16" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2">
				{#if collapsed}
					<polyline points="9 18 15 12 9 6" />
				{:else}
					<polyline points="15 18 9 12 15 6" />
				{/if}
			</svg>
		</button>
	</div>

	<nav class="sidebar-nav">
		{#each nav as item}
			<a
				href={item.href}
				class="nav-item"
				class:active={isActive(item.href)}
				title={collapsed && !mobileOpen ? item.label : undefined}
				onclick={onMobileClose}
			>
				<span class="nav-icon">
					{#if item.icon === 'grid'}
						<svg width="18" height="18" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2"><rect x="3" y="3" width="7" height="7" rx="1"/><rect x="14" y="3" width="7" height="7" rx="1"/><rect x="3" y="14" width="7" height="7" rx="1"/><rect x="14" y="14" width="7" height="7" rx="1"/></svg>
					{:else if item.icon === 'wallet'}
						<svg width="18" height="18" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2"><path d="M21 12V7H5a2 2 0 0 1 0-4h14v4"/><path d="M3 5v14a2 2 0 0 0 2 2h16v-5"/><circle cx="18" cy="16" r="1"/></svg>
					{:else if item.icon === 'zap'}
						<svg width="18" height="18" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2"><polygon points="13 2 3 14 12 14 11 22 21 10 12 10 13 2"/></svg>
					{:else if item.icon === 'radio'}
						<svg width="18" height="18" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2"><circle cx="12" cy="12" r="2"/><path d="M16.24 7.76a6 6 0 0 1 0 8.49m-8.48-.01a6 6 0 0 1 0-8.49m11.31-2.82a10 10 0 0 1 0 14.14m-14.14 0a10 10 0 0 1 0-14.14"/></svg>
					{:else if item.icon === 'globe'}
						<svg width="18" height="18" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2"><circle cx="12" cy="12" r="10"/><line x1="2" y1="12" x2="22" y2="12"/><path d="M12 2a15.3 15.3 0 0 1 4 10 15.3 15.3 0 0 1-4 10 15.3 15.3 0 0 1-4-10 15.3 15.3 0 0 1 4-10z"/></svg>
					{:else if item.icon === 'settings'}
						<svg width="18" height="18" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2"><circle cx="12" cy="12" r="3"/><path d="M19.4 15a1.65 1.65 0 0 0 .33 1.82l.06.06a2 2 0 0 1-2.83 2.83l-.06-.06a1.65 1.65 0 0 0-1.82-.33 1.65 1.65 0 0 0-1 1.51V21a2 2 0 0 1-4 0v-.09A1.65 1.65 0 0 0 9 19.4a1.65 1.65 0 0 0-1.82.33l-.06.06a2 2 0 0 1-2.83-2.83l.06-.06A1.65 1.65 0 0 0 4.68 15a1.65 1.65 0 0 0-1.51-1H3a2 2 0 0 1 0-4h.09A1.65 1.65 0 0 0 4.6 9a1.65 1.65 0 0 0-.33-1.82l-.06-.06a2 2 0 0 1 2.83-2.83l.06.06A1.65 1.65 0 0 0 9 4.68a1.65 1.65 0 0 0 1-1.51V3a2 2 0 0 1 4 0v.09a1.65 1.65 0 0 0 1 1.51 1.65 1.65 0 0 0 1.82-.33l.06-.06a2 2 0 0 1 2.83 2.83l-.06.06A1.65 1.65 0 0 0 19.4 9a1.65 1.65 0 0 0 1.51 1H21a2 2 0 0 1 0 4h-.09a1.65 1.65 0 0 0-1.51 1z"/></svg>
					{/if}
				</span>
				{#if !collapsed || mobileOpen}
					<span class="nav-label">{item.label}</span>
				{/if}
			</a>
		{/each}
	</nav>

	<div class="sidebar-footer">
		<div class="connection-status">
			<span class="status-dot" class:online={connected}></span>
			{#if !collapsed || mobileOpen}
				<span class="status-text">{connected ? 'Connected' : 'Disconnected'}</span>
			{/if}
		</div>
	</div>
</aside>

<style>
	.sidebar {
		position: fixed;
		top: 0;
		left: 0;
		bottom: 0;
		width: var(--sidebar-width);
		background: var(--bg-surface);
		border-right: 1px solid var(--border-dim);
		display: flex;
		flex-direction: column;
		z-index: 50;
		transition: width 0.3s var(--ease-out-expo);
	}
	.sidebar.collapsed {
		width: 64px;
	}
	.sidebar-header {
		display: flex;
		align-items: center;
		justify-content: space-between;
		padding: 1rem;
		height: var(--topbar-height);
		border-bottom: 1px solid var(--border-dim);
	}
	.logo {
		display: flex;
		align-items: center;
		gap: 0.5rem;
		text-decoration: none;
	}
	.wolf-icon {
		border-radius: 6px;
		box-shadow: 0 0 12px rgba(255, 153, 0, 0.3);
		border: 1px solid rgba(255, 153, 0, 0.2);
		flex-shrink: 0;
	}
	.logo-text {
		font-family: var(--font-mono);
		font-weight: 700;
		font-size: 0.9rem;
		color: var(--text-accent);
		letter-spacing: -0.02em;
		white-space: nowrap;
	}
	.collapse-btn {
		background: none;
		border: none;
		color: var(--text-muted);
		cursor: pointer;
		padding: 0.25rem;
		display: flex;
		transition: color 0.2s;
	}
	.collapse-btn:hover { color: var(--text-primary); }

	.sidebar-nav {
		flex: 1;
		padding: 0.75rem 0.5rem;
		display: flex;
		flex-direction: column;
		gap: 2px;
		overflow-y: auto;
	}
	.nav-item {
		display: flex;
		align-items: center;
		gap: 0.75rem;
		padding: 0.6rem 0.75rem;
		border-radius: 8px;
		color: var(--text-secondary);
		text-decoration: none;
		font-family: var(--font-mono);
		font-size: 0.8rem;
		transition: all 0.2s;
		position: relative;
	}
	.nav-item:hover {
		color: var(--text-primary);
		background: var(--orange-glow);
	}
	.nav-item.active {
		color: var(--text-accent);
		background: var(--orange-glow-strong);
	}
	.nav-item.active::before {
		content: '';
		position: absolute;
		left: 0;
		top: 50%;
		transform: translateY(-50%);
		width: 3px;
		height: 20px;
		background: var(--text-accent);
		border-radius: 0 3px 3px 0;
	}
	.nav-icon {
		display: flex;
		align-items: center;
		flex-shrink: 0;
	}
	.nav-label {
		white-space: nowrap;
	}

	.sidebar-footer {
		padding: 1rem;
		border-top: 1px solid var(--border-dim);
	}
	.connection-status {
		display: flex;
		align-items: center;
		gap: 0.5rem;
	}
	.status-dot {
		width: 8px;
		height: 8px;
		border-radius: 50%;
		background: var(--color-error);
		flex-shrink: 0;
	}
	.status-dot.online {
		background: var(--color-success);
		box-shadow: 0 0 8px rgba(52, 211, 153, 0.5);
	}
	.status-text {
		font-family: var(--font-mono);
		font-size: 0.7rem;
		color: var(--text-muted);
	}

	.mobile-backdrop {
		display: none;
	}

	@media (max-width: 768px) {
		.mobile-backdrop {
			display: block;
			position: fixed;
			inset: 0;
			background: rgba(0, 0, 0, 0.6);
			z-index: 49;
		}
		.sidebar {
			width: 64px;
			transform: translateX(-100%);
		}
		.sidebar.mobile-open {
			transform: translateX(0);
			width: var(--sidebar-width);
		}
		.logo-text, .nav-label, .status-text {
			display: none;
		}
		.sidebar.mobile-open .logo-text,
		.sidebar.mobile-open .nav-label,
		.sidebar.mobile-open .status-text {
			display: inline;
		}
		.collapse-btn {
			display: none;
		}
	}
</style>
