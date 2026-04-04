import '@testing-library/jest-dom/vitest';

// Mock localStorage
const store: Record<string, string> = {};
const localStorageMock = {
	getItem: (key: string) => store[key] ?? null,
	setItem: (key: string, value: string) => { store[key] = value; },
	removeItem: (key: string) => { delete store[key]; },
	clear: () => { Object.keys(store).forEach(k => delete store[k]); },
	get length() { return Object.keys(store).length; },
	key: (i: number) => Object.keys(store)[i] ?? null,
};
Object.defineProperty(globalThis, 'localStorage', { value: localStorageMock });

// Mock clipboard
Object.defineProperty(navigator, 'clipboard', {
	value: {
		writeText: vi.fn().mockResolvedValue(undefined),
		readText: vi.fn().mockResolvedValue(''),
	},
	writable: true,
});

// Mock fetch
globalThis.fetch = vi.fn();

// Reset between tests
beforeEach(() => {
	localStorageMock.clear();
	vi.clearAllMocks();
});
