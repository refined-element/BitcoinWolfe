import QRCode from 'qrcode';

export async function generateQrDataUrl(data: string): Promise<string> {
	return QRCode.toDataURL(data, {
		width: 256,
		margin: 2,
		color: {
			dark: '#e8e4de',
			light: '#0d0f12'
		}
	});
}
