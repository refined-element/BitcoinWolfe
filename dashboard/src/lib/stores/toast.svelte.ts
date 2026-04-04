export interface ToastItem {
	id: number;
	message: string;
	type: 'success' | 'error' | 'info';
}

let toasts = $state<ToastItem[]>([]);
let nextId = 0;

export function showToast(message: string, type: ToastItem['type'] = 'info') {
	const id = nextId++;
	toasts = [...toasts, { id, message, type }];
	setTimeout(() => {
		toasts = toasts.filter(t => t.id !== id);
	}, 4000);
}

export function getToasts() {
	return {
		get list() { return toasts; }
	};
}
