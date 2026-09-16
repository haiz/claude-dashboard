// Ports DashboardViewModel's reset monitor: the rule that decides when an
// account's saved command fires by itself, and the once-per-episode latch that
// stops it re-firing on every refresh.
//
// The signal is not "a window reset" but "a window has no reset time at all".
// The API drops `resets_at` in the gap between one cycle ending and the next
// starting — "the circle has not started" — and that gap is what macOS watches
// for (DashboardViewModel.shouldRunSavedCommand, lines 277-286).

// shouldRunSavedCommand(for:) — either window missing its reset time is enough.
export function shouldRunSavedCommand(windows) {
    if (!windows)
        return false;
    return windows.fiveHour?.resetsAtMs === null || windows.sevenDay?.resetsAtMs === null;
}

// The `pingedAccounts` latch. An account fires once when it enters the episode
// and is only re-armed once both windows report a reset again — otherwise the
// command would run on every poll for as long as the gap lasted, and the
// command log would fill with repeats.
export class AutoRunLatch {
    constructor() {
        this._pinged = new Set();
    }

    // Returns the ids that should run their saved command on this pass.
    due(rows) {
        const out = [];
        for (const row of rows) {
            if (shouldRunSavedCommand(row.windows)) {
                if (!this._pinged.has(row.id)) {
                    this._pinged.add(row.id);
                    out.push(row.id);
                }
            } else {
                this._pinged.delete(row.id);
            }
        }
        // An account that vanished must not hold the latch forever.
        const live = new Set(rows.map(row => row.id));
        for (const id of [...this._pinged]) {
            if (!live.has(id))
                this._pinged.delete(id);
        }
        return out;
    }

    get armedCount() {
        return this._pinged.size;
    }
}
