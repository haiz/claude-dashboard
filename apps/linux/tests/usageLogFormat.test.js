import GLib from 'gi://GLib';
import {test, assertEqual} from './harness.js';
import {UsageLog, serialize} from '../lib/usageLog.js';

const REPO = GLib.getenv('CLAUDE_DASHBOARD_REPO') ?? '../..';

function loadFixture() {
    const [ok, bytes] = GLib.file_get_contents(`${REPO}/contract/cases/linux-usage-log.json`);
    if (!ok)
        throw new Error('cannot read contract case linux-usage-log.json');
    return JSON.parse(new TextDecoder().decode(bytes));
}

test('the JS writer reproduces the pinned document byte for byte', () => {
    const {recordings, serialized} = loadFixture();
    const log = new UsageLog();
    for (const r of recordings)
        log.record(r);
    assertEqual(serialize(log), serialized);
});

test('the pinned document compresses a plateau to first and last', () => {
    const {serialized} = loadFixture();
    const rows = JSON.parse(serialized).rows;
    const plateau = rows.filter(r => r.aid === 'acc-1' && r.w === 0 && r.rat === 1789430000 && r.u === 500);
    assertEqual(plateau.length, 2);
});
