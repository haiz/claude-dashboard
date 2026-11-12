// Free "limit resets" (a Full reset and Fable resets, each with an expiry),
// read from the usage payload. Pure: no gi:// or GNOME Shell imports.
//
// The payload carries them under an unreleased internal field name, called
// "the limit-resets field" everywhere in this repo. The name is never stored
// raw (scripts/test-no-codename.sh enforces it): it is kept base64-encoded and
// decoded at runtime by fieldName(). The usage request must carry
// `?<fieldName()>=1` for the field to be filled. The app only displays
// resets; it never calls the reset endpoint.
//
// Rules: contract/README.md "Limit resets"; cases in
// contract/cases/limit-resets.json.
//
//   summarize(parseGrants(null), 0)  // -> {full: null, fable: null}

import {parseTimestamp} from './model.js';

export const ENCODED_FIELD_NAME = 'Y2VkYXJfZW1iZXI=';

// A reset expiring in less than this many ms is urgent (7 days).
export const URGENT_MS = 604800 * 1000;

const U32_MAX = 4294967295;
const B64 = 'ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/';

// Hand-written base64 decoder (ASCII output only; atob is not relied on).
function decodeBase64(text) {
    let bits = 0, acc = 0, out = '';
    for (const ch of text) {
        if (ch === '=')
            break;
        acc = (acc << 6) | B64.indexOf(ch);
        bits += 6;
        if (bits >= 8) {
            bits -= 8;
            out += String.fromCharCode((acc >> bits) & 0xff);
            acc &= (1 << bits) - 1;
        }
    }
    return out;
}

// The decoded name of the limit-resets field (query parameter and JSON key).
export function fieldName() {
    return decodeBase64(ENCODED_FIELD_NAME);
}

function isCount(x) {
    return typeof x === 'number' && Number.isInteger(x) && x >= 1 && x <= U32_MAX;
}

function parseGrant(g) {
    if (g === null || typeof g !== 'object' || Array.isArray(g))
        return null;
    if (!isCount(g.resets_left) || g.paused === true)
        return null;
    if (typeof g.ends_at !== 'string')
        return null;
    const endsAtMs = parseTimestamp(g.ends_at);
    if (endsAtMs === null)
        return null;
    let kind;
    if (g.gives_back_weekly === true) {
        kind = 'fable';
    } else if (Array.isArray(g.clears) && g.clears.includes('five_hour') && g.clears.includes('seven_day')) {
        kind = 'full';
    } else {
        return null;
    }
    return {kind, resetsLeft: g.resets_left, endsAtMs};
}

// Time-independent: keeps every well-formed, unpaused grant with
// resets_left >= 1. Malformed input degrades to fewer grants, never an error.
export function parseGrants(obj) {
    if (obj === null || typeof obj !== 'object' || Array.isArray(obj))
        return [];
    if (obj.eligible !== true || !Array.isArray(obj.grants))
        return [];
    return obj.grants.map(parseGrant).filter(g => g !== null);
}

// Drops grants that ended at or before nowMs, then sums counts (capped at the
// u32 max) and takes the earliest expiry per kind.
export function summarize(grants, nowMs) {
    const of = kind => {
        const live = grants.filter(g => g.kind === kind && g.endsAtMs > nowMs);
        if (live.length === 0)
            return null;
        const expiresAtMs = Math.min(...live.map(g => g.endsAtMs));
        const count = Math.min(U32_MAX, live.reduce((a, g) => a + g.resetsLeft, 0));
        return {count, expiresAtMs, urgent: expiresAtMs - nowMs < URGENT_MS};
    };
    return {full: of('full'), fable: of('fable')};
}
