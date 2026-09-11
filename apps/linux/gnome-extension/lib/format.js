// Ports UsageBar.formatResetTime and UsageBar.formattedCountdown
// (apps/macos/ClaudeDashboard/Views/UsageBar.swift:136 and :158).

const FIVE_HOUR_SECONDS = 18000;

function pad2(n) {
    return String(n).padStart(2, '0');
}

export function formattedCountdown(remainingSeconds, totalSeconds) {
    if (!(remainingSeconds > 0))
        return '0:00';
    const total = Math.floor(remainingSeconds);
    if (totalSeconds <= FIVE_HOUR_SECONDS) {
        const hours = Math.floor(total / 3600);
        const minutes = Math.floor((total % 3600) / 60);
        const seconds = total % 60;
        return hours > 0 ? `${hours}:${pad2(minutes)}` : `${minutes}:${pad2(seconds)}`;
    }
    const days = Math.floor(total / 86400);
    const hours = Math.floor((total % 86400) / 3600);
    const minutes = Math.floor((total % 3600) / 60);
    return days > 0 ? `${days}d${hours}h` : `${hours}:${pad2(minutes)}`;
}

export function formatResetTime(date, totalSeconds, now = new Date()) {
    if (date.getTime() <= now.getTime())
        return 'now';

    if (totalSeconds <= FIVE_HOUR_SECONDS)
        return date.toLocaleTimeString(undefined, {hour: 'numeric', minute: '2-digit'});

    // The seven-day window resets off the hour. Rounding to the nearest ten
    // minutes keeps the wall-clock label agreeing with the countdown instead
    // of truncating, so 23:59 reads as the next midnight rather than 11pm.
    const minutesFloat = date.getMinutes() + date.getSeconds() / 60;
    const deltaMinutes = Math.round(minutesFloat / 10) * 10 - minutesFloat;
    const rounded = new Date(date.getTime() + deltaMinutes * 60000);

    const weekday = rounded.toLocaleDateString(undefined, {weekday: 'short'});
    let hour = rounded.getHours() % 12;
    if (hour === 0)
        hour = 12;
    const suffix = rounded.getHours() < 12 ? 'am' : 'pm';
    const minute = rounded.getMinutes();
    const clock = minute === 0 ? `${hour}${suffix}` : `${hour}:${pad2(minute)}${suffix}`;

    const raw = `${weekday} ${clock}`.toLowerCase();
    return raw.charAt(0).toUpperCase() + raw.slice(1);
}
