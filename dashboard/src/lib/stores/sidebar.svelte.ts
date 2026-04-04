let mobileOpen = $state(false);

export function sidebarStore() {
	return {
		get mobileOpen() { return mobileOpen; },
		toggle() { mobileOpen = !mobileOpen; },
		close() { mobileOpen = false; },
	};
}
