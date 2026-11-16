import assert from 'node:assert/strict';
import test from 'node:test';

globalThis.window = globalThis.window || {};
const levels = await import('../../frontend/levels.js');

test('built-in Zephyr detection recognises log and bracket tags, ignoring ANSI', () => {
    const { detectZephyrLevel, NO_LEVEL } = levels;
    assert.equal(detectZephyrLevel('[00:00:01.000,000] <err> main: boom'), 'err');
    assert.equal(detectZephyrLevel('\x1b[1;31m[00:00:01.000,000] <wrn> net: slow\x1b[0m'), 'wrn');
    assert.equal(detectZephyrLevel('<INF> boot'), 'inf');
    assert.equal(detectZephyrLevel('[DEBUG] verbose'), 'dbg');
    assert.equal(detectZephyrLevel('[warning] x'), 'wrn');
    assert.equal(detectZephyrLevel('uart:~$ help'), NO_LEVEL);
    assert.equal(detectZephyrLevel(''), NO_LEVEL);
});

test('entry level prefers server levels, skips TX, and falls back to Zephyr only without server profiles', () => {
    const { entryLevel, NO_LEVEL, refreshLevelDefinitions, paneLevelDefs } = levels;
    delete window.__embedLogSourceLevels;
    refreshLevelDefinitions();
    assert.equal(entryLevel({ rawText: '<dbg> x', isTx: false }), 'dbg');
    assert.equal(entryLevel({ rawText: '<dbg> x', isTx: true }), null);
    assert.equal(entryLevel(['', 'plain', false, { level: 'trace' }]), 'trace');
    assert.deepEqual(paneLevelDefs('A').map(d => [d.id, d.defaultVisible]),
        [['err', true], ['wrn', true], ['inf', true], ['dbg', false], [NO_LEVEL, true]]);

    window.__embedLogSourceLevels = {
        A: { levels: [{ id: 'fatal', label: 'Fatal' }, { id: 'trace', default_visible: false }] },
        B: null,
    };
    refreshLevelDefinitions();
    assert.equal(entryLevel({ rawText: '<dbg> x', isTx: false }), NO_LEVEL);
    assert.deepEqual(paneLevelDefs('A').map(d => d.label), ['Fatal', 'trace', 'No level']);
    assert.deepEqual(paneLevelDefs('B'), []);
    delete window.__embedLogSourceLevels;
    refreshLevelDefinitions();
});
