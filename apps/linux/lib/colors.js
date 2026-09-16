// Ports DashboardViewModel.usageColor and UsageBar.segmentColor
// (apps/macos/ClaudeDashboard/ViewModels/DashboardViewModel.swift:603,
//  apps/macos/ClaudeDashboard/Views/UsageBar.swift:64).
// SwiftUI's Color(hue:saturation:brightness:) is HSV, so this is a plain HSV
// conversion, not HSL.

const COUNTDOWN_BLUE = {r: 74 / 255, g: 144 / 255, b: 217 / 255};

export function hsvToRgb(hue, saturation, value) {
    const h = (hue % 1 + 1) % 1;
    const i = Math.floor(h * 6);
    const f = h * 6 - i;
    const p = value * (1 - saturation);
    const q = value * (1 - f * saturation);
    const t = value * (1 - (1 - f) * saturation);
    switch (i % 6) {
    case 0: return {r: value, g: t, b: p};
    case 1: return {r: q, g: value, b: p};
    case 2: return {r: p, g: value, b: t};
    case 3: return {r: p, g: q, b: value};
    case 4: return {r: t, g: p, b: value};
    default: return {r: value, g: p, b: q};
    }
}

export function usageColor(utilization) {
    const hue = Math.max(0, Math.min(120, 120 * (1 - utilization / 100))) / 360;
    return hsvToRgb(hue, 0.7, 0.85);
}

export function countdownColor(remainingSeconds, totalSeconds) {
    if (!(remainingSeconds > 0) || !(totalSeconds > 0))
        return hsvToRgb(120 / 360, 0.7, 0.85);
    const fraction = Math.min(remainingSeconds / totalSeconds, 1.0);
    if (fraction > 0.3)
        return COUNTDOWN_BLUE;
    const greenIntensity = 1.0 - fraction / 0.3;
    return hsvToRgb(120 / 360, 0.6 * greenIntensity + 0.1, 0.5 + 0.35 * greenIntensity);
}

export const BURN_ANIMALS = {1: '🐌', 2: '🐢', 3: '🐇', 4: '🐎', 5: '🐆'};

// Every comparison is strictly greater-than; contract/cases/burn-rate-levels.json
// pins the boundaries (exactly 5h is level 2, exactly 3h is level 3, and so on).
export function burnRateLevel(projectedSeconds) {
    const hours = projectedSeconds / 3600;
    if (hours > 5) return 1;
    if (hours > 3) return 2;
    if (hours > 1.5) return 3;
    if (hours > 0.5) return 4;
    return 5;
}

export function burnRateAnimal(projectedSeconds) {
    return BURN_ANIMALS[burnRateLevel(projectedSeconds)];
}
