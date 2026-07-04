export function nowUnix() {
	return Math.floor(Date.now() / 1000);
}

export function usageDate(daysAgo) {
	const date = new Date(Date.now() - daysAgo * 86_400_000);

	return date.toISOString().slice(0, 10);
}

export function profileDailyUsage(values = []) {
	const days = values.length;
	return values.map((tokens, index) => ({
		date: usageDate(days - index - 1),
		tokens,
	}));
}

export function unixToIso(seconds) {
	return new Date(seconds * 1000).toISOString();
}

export function ago(seconds) {
	return unixToIso(nowUnix() - seconds);
}

export function later(seconds) {
	return unixToIso(nowUnix() + seconds);
}

export function unixLater(seconds) {
	return nowUnix() + seconds;
}

