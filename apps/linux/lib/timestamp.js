// Leaf module: ISO8601 timestamp parsing shared by model.js and limitResets.js
// (kept apart so those two never import each other).

// The API emits both fractional and whole-second ISO8601. Date.parse accepts
// both forms, including six-digit fractions. Date-only, zone-less and free-form
// strings are rejected, as in Rust and Swift.
//
//   parseTimestamp('2026-11-12T00:00:00Z')  // -> 1794441600000
const RFC3339 = /^\d{4}-\d{2}-\d{2}T\d{2}:\d{2}:\d{2}(\.\d+)?(Z|[+-]\d{2}:\d{2})$/;

export function parseTimestamp(value) {
    if (value === null || value === undefined)
        return null;
    if (typeof value !== 'string' || !RFC3339.test(value))
        return null;
    const ms = Date.parse(value);
    return Number.isNaN(ms) ? null : ms;
}
