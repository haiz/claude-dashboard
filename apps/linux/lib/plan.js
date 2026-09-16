// Ports AccountBadgeColor.swift: the plan pill is tinted per plan, not one
// fixed colour. macOS uses the system palette at 15% behind the default label
// colour (AccountCard.swift:63-69); these are those system colours in sRGB.
const PLAN_COLORS = {
    'Pro': 'rgba(0, 122, 255, 0.15)',        // .blue
    'Max 5x': 'rgba(175, 82, 222, 0.15)',    // .purple
    'Max 20x': 'rgba(255, 159, 10, 0.15)',   // .orange
    'Max': 'rgba(175, 82, 222, 0.15)',       // .purple — max200 on macOS
};

// An unrecognised wire value keeps the generic purple rather than rendering
// an untinted pill, so a new plan string degrades to "some Max-ish plan"
// instead of looking like a rendering bug.
const FALLBACK = 'rgba(175, 82, 222, 0.15)';

export function planBadgeColor(plan) {
    return PLAN_COLORS[plan] ?? FALLBACK;
}
