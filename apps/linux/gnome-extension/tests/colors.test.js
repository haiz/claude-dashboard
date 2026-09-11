import GLib from 'gi://GLib';
import {test, assertEqual, assertClose} from './harness.js';
import {usageColor, countdownColor, burnRateLevel, burnRateAnimal} from '../lib/colors.js';

const REPO = GLib.getenv('CLAUDE_DASHBOARD_REPO') ?? '../../..';

function loadCases(name) {
    const [ok, bytes] = GLib.file_get_contents(`${REPO}/contract/cases/${name}`);
    if (!ok)
        throw new Error(`cannot read contract case ${name}`);
    return JSON.parse(new TextDecoder().decode(bytes));
}

test('usageColor is green at 0 percent', () => {
    const c = usageColor(0);
    assertClose(c.r, 0.255, 0.002, 'r');
    assertClose(c.g, 0.85, 0.002, 'g');
    assertClose(c.b, 0.255, 0.002, 'b');
});

test('usageColor is red at 100 percent', () => {
    const c = usageColor(100);
    assertClose(c.r, 0.85, 0.002, 'r');
    assertClose(c.g, 0.255, 0.002, 'g');
    assertClose(c.b, 0.255, 0.002, 'b');
});

test('usageColor clamps above 100 percent', () => {
    const a = usageColor(100), b = usageColor(140);
    assertClose(b.r, a.r, 1e-9, 'r');
    assertClose(b.g, a.g, 1e-9, 'g');
});

test('countdownColor stays blue above 30 percent remaining', () => {
    const c = countdownColor(0.5 * 18000, 18000);
    assertClose(c.r, 74 / 255, 1e-9, 'r');
    assertClose(c.g, 144 / 255, 1e-9, 'g');
    assertClose(c.b, 217 / 255, 1e-9, 'b');
});

test('countdownColor crosses toward green below 30 percent remaining', () => {
    const c = countdownColor(0.15 * 18000, 18000);
    if (!(c.g > c.r && c.g > c.b))
        throw new Error(`expected a green-dominant colour, got ${JSON.stringify(c)}`);
});

test('burn rate levels match the contract cases', () => {
    for (const c of loadCases('burn-rate-levels.json')) {
        assertEqual(burnRateLevel(c.projected_seconds), c.expect_level, c.name);
        assertEqual(burnRateAnimal(c.projected_seconds), c.expect_animal, c.name);
    }
});
