import { defineConfig } from 'vitest/config';
import { svelte } from '@sveltejs/vite-plugin-svelte';

export default defineConfig({
	plugins: [svelte({ hot: false })],
	resolve: {
		conditions: ['browser']
	},
	test: {
		environment: 'jsdom',
		globals: true,
		setupFiles: ['./src/tests/setup.ts'],
		include: ['src/**/*.test.ts'],
		alias: {
			'$lib': '/src/lib',
			'$app/state': '/src/tests/mocks/app-state.ts',
			'$app/stores': '/src/tests/mocks/app-stores.ts'
		}
	}
});
