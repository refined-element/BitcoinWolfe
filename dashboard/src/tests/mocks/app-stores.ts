// Mock $app/stores for tests (fallback if anything uses legacy stores)
export const page = {
	subscribe: (fn: (val: any) => void) => {
		fn({
			url: new URL('http://localhost/'),
			params: {},
			route: { id: '/' },
			status: 200,
			error: null,
			data: {},
		});
		return () => {};
	}
};
