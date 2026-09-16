import {test, assertEqual} from './harness.js';
import {shouldRunSavedCommand, AutoRunLatch} from '../lib/autoRun.js';

function windows(fiveHourReset, sevenDayReset) {
    return {
        fiveHour: {utilization: 0, resetsAtMs: fiveHourReset},
        sevenDay: {utilization: 0, resetsAtMs: sevenDayReset},
    };
}

test('the trigger is a missing reset time, not a reset itself', () => {
    assertEqual(shouldRunSavedCommand(windows(1, 2)), false);
    assertEqual(shouldRunSavedCommand(windows(null, 2)), true);
    assertEqual(shouldRunSavedCommand(windows(1, null)), true);
    assertEqual(shouldRunSavedCommand(null), false);
});

test('an account fires once per episode, not once per poll', () => {
    const latch = new AutoRunLatch();
    const rows = [{id: 'a', windows: windows(null, 2)}];
    assertEqual(latch.due(rows).join(','), 'a');
    assertEqual(latch.due(rows).join(','), '');
    assertEqual(latch.due(rows).join(','), '');
});

test('it re-arms once both windows report a reset again', () => {
    const latch = new AutoRunLatch();
    assertEqual(latch.due([{id: 'a', windows: windows(null, 2)}]).join(','), 'a');
    assertEqual(latch.due([{id: 'a', windows: windows(1, 2)}]).join(','), '');
    assertEqual(latch.due([{id: 'a', windows: windows(null, 2)}]).join(','), 'a');
});

test('accounts latch independently', () => {
    const latch = new AutoRunLatch();
    const due = latch.due([
        {id: 'a', windows: windows(null, 2)},
        {id: 'b', windows: windows(1, 2)},
    ]);
    assertEqual(due.join(','), 'a');
    assertEqual(latch.due([
        {id: 'a', windows: windows(null, 2)},
        {id: 'b', windows: windows(null, 2)},
    ]).join(','), 'b');
});

test('a vanished account releases its latch', () => {
    const latch = new AutoRunLatch();
    latch.due([{id: 'a', windows: windows(null, 2)}]);
    assertEqual(latch.armedCount, 1);
    latch.due([]);
    assertEqual(latch.armedCount, 0);
});

test('a row with no usage at all never fires', () => {
    const latch = new AutoRunLatch();
    assertEqual(latch.due([{id: 'a', windows: null}]).join(','), '');
});
