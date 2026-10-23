import assert from 'node:assert/strict';
import test from 'node:test';
import { isErasedConsoleLine, parseAnsi } from '../../frontend/ansi.js';

test('fully erased terminal redraws are transient, without prompt-name matching', () => {
    for (const raw of [
        'device> \x1b[8D\x1b[J',
        'device uart> \x1b[13D\x1b[J',
        '\x1b[32mdevice> \x1b[0m\x1b[8D\x1b[0K',
        'typed command\r\x1b[K',
        'arbitrary text\x1b[2K',
        'abc\x1b[1G\x1b[J',
        'abc\b\b\b\x1b[J',
        'abc\x1b[3D\x1b[3C\x1b[1K',
    ]) assert.equal(isErasedConsoleLine(raw), true, JSON.stringify(raw));
});

test('control-only erase sequences are transient', () => {
    for (const raw of [
        '\x1b[0K',
        '\x1b[2K',
        '\x1b[0K\x1b[0m',
    ]) assert.equal(isErasedConsoleLine(raw), true, JSON.stringify(raw));
});

test('preserve ordinary console, command responses, partial erasure and new output', () => {
    for (const raw of [
        '', 'device> ', 'device> help', 'deadbeef', '  help',
        'Available commands:', '[00000001] <inf> app: ok',
        '\x1b[32mdevice> \x1b[0m',
        'device> \x1b[3D\x1b[J',
        'device> \x1b[8D\x1b[Jresponse',
        'abc\x1b[3D\x1b[Kok',
        'abc\n\x1b[2K',
    ]) assert.equal(isErasedConsoleLine(raw), false, JSON.stringify(raw));
});

test('unknown controls, uncertain character widths and excessive cursors stay visible', () => {
    for (const raw of [
        '\u{1f512}> \x1b[4D\x1b[J',
        'abc\x1b[2A\x1b[J',
        'abc\x1b[999999999C\x1b[J',
        'abc\x1b[3;2D\x1b[J',
        'abc\t\x1b[2K', 'abc\x1b[',
    ]) assert.equal(isErasedConsoleLine(raw), false, JSON.stringify(raw));
});

test('classification does not alter raw bytes or existing safe ANSI rendering', () => {
    const raw = 'device> \x1b[8D\x1b[J';
    assert.equal(isErasedConsoleLine(raw), true);
    assert.equal(raw, 'device> \x1b[8D\x1b[J');
    assert.equal(parseAnsi('<script>\x1b[31merr\x1b[0m'), '&lt;script&gt;<span class="ansi-1">err</span>');
});
