const cases = [];
let failures = 0;

export function test(name, fn) {
    cases.push({name, fn});
}

export function assertEqual(actual, expected, msg = '') {
    if (actual !== expected)
        throw new Error(`${msg} expected ${JSON.stringify(expected)}, got ${JSON.stringify(actual)}`);
}

export function assertClose(actual, expected, tolerance, msg = '') {
    if (!(Math.abs(actual - expected) <= tolerance))
        throw new Error(`${msg} expected ${expected} +/- ${tolerance}, got ${actual}`);
}

export function assertDeepEqual(actual, expected, msg = '') {
    const a = JSON.stringify(actual), b = JSON.stringify(expected);
    if (a !== b)
        throw new Error(`${msg} expected ${b}, got ${a}`);
}

export function runAll() {
    for (const {name, fn} of cases) {
        try {
            fn();
            print(`ok   ${name}`);
        } catch (e) {
            failures += 1;
            print(`FAIL ${name}: ${e.message}`);
        }
    }
    print(`\n${cases.length - failures} passed, ${failures} failed`);
    return failures === 0 ? 0 : 1;
}
