import assert from 'node:assert/strict';
import test from 'node:test';
import { PhraseSearch } from '../../frontend/search.js';

const row = (text, tx = false) => ['', text, tx, {}];

test('literal case-insensitive search counts occurrences, including ANSI and Unicode offsets', () => {
    const search = new PhraseSearch('a.b');
    search.update([row('A.B aXb \x1b[31ma.b\x1b[0m'), row('😀a.b')]);
    assert.deepEqual(search.matches, [
        { row: 0, start: 0, end: 3 }, { row: 0, start: 8, end: 11 },
        { row: 1, start: 2, end: 5 },
    ]);
    assert.deepEqual(search.move(1), search.matches[0]);
    search.move(-1);
    assert.equal(search.cursor, 2);
    search.move(1);
    assert.equal(search.cursor, 0);
    const empty = new PhraseSearch('');
    empty.update([row('anything')]);
    assert.equal(empty.move(1), null);
});

test('incremental appends keep the cursor; replacing or clearing history resets it', () => {
    const search = new PhraseSearch('hit');
    const rows = [row('hit hit')];
    search.update(rows);
    search.move(1);
    rows.push({ rawText: 'HIT', isTx: false });
    search.update(rows);
    search.update(rows);
    assert.equal(search.matches.length, 3);
    assert.equal(search.cursor, 0);
    search.update([row('hit'), ...rows]);
    assert.equal(search.matches.length, 4);
    assert.equal(search.cursor, -1);
    search.update([]);
    assert.equal(search.matches.length, 0);
});

test('erased console redraws do not count, while TX text and regex punctuation remain literal', () => {
    const search = new PhraseSearch('abc');
    search.update([row('abc\x1b[2K'), row('abc\x1b[2K', true)]);
    assert.equal(search.matches.length, 1);
    assert.equal(search.matches[0].row, 1);
    const punctuation = new PhraseSearch('[a-z]+($)\\');
    punctuation.update([row('[a-z]+($)\\')]);
    assert.equal(punctuation.matches.length, 1);
});
