import assert from 'node:assert/strict';
import test from 'node:test';
import {
  stripDuplicateLeadingTimestamp,
  unpadBracketLevel,
  stripUptimeCounter,
  denoiseMessage,
  elapsedTime,
  ShortcodeTable,
  estimateTokens,
} from '../../frontend/postprocess.js';

// Real strings observed in a live session (2026-07-06_14-31-18), same
// fixtures as crates/embed-log-core/src/postprocess.rs's Rust tests — this
// module is a hand-kept JS mirror of that one.

test('stripDuplicateLeadingTimestamp removes a matching prefix', () => {
  const msg = "15:41:23.644 [   ERROR] Timeout waiting for event='dcf_edhoc'";
  assert.equal(
    stripDuplicateLeadingTimestamp(msg, '15:41:23.644'),
    "[   ERROR] Timeout waiting for event='dcf_edhoc'",
  );
});

test('stripDuplicateLeadingTimestamp leaves a mismatched prefix', () => {
  const msg = '15:41:23.644 something happened at a different logged time';
  assert.equal(stripDuplicateLeadingTimestamp(msg, '09:00:00.000'), msg);
});

test('unpadBracketLevel collapses padding', () => {
  assert.equal(unpadBracketLevel('[   ERROR] boom'), '[ERROR] boom');
  assert.equal(unpadBracketLevel('[INFO] fine'), '[INFO] fine');
});

test('stripUptimeCounter removes the counter, keeps the level tag', () => {
  const msg = '[00000002] <inf> flash_stm32_ospi: Read SFDP from octoFlash';
  assert.equal(stripUptimeCounter(msg), '<inf> flash_stm32_ospi: Read SFDP from octoFlash');
});

test('stripUptimeCounter ignores a non-tag bracket', () => {
  const msg = '[00000002] not a level tag';
  assert.equal(stripUptimeCounter(msg), msg);
});

test('denoiseMessage applies both steps in order', () => {
  const msg = "15:41:23.644 [   ERROR] Timeout waiting for event='dcf_edhoc'";
  assert.equal(
    denoiseMessage(msg, '15:41:23.644'),
    "[ERROR] Timeout waiting for event='dcf_edhoc'",
  );
});

test('elapsedTime formats by magnitude', () => {
  assert.equal(elapsedTime({ relNum: 644 }, '?'), '0.644');
  assert.equal(elapsedTime({ relNum: 83_644 }, '?'), '1:23.644');
  assert.equal(elapsedTime({ relNum: 3_723_644 }, '?'), '1:02:03.644');
});

test('elapsedTime falls back when relNum is missing', () => {
  assert.equal(elapsedTime({}, '15:41:23.644'), '15:41:23.644');
});

test('ShortcodeTable derives meaningful initials and reuses them', () => {
  const codes = new ShortcodeTable();
  assert.equal(codes.codeFor('PYTEST'), 'P');
  assert.equal(codes.codeFor('COUNTER'), 'C');
  assert.equal(codes.codeFor('PYTEST'), 'P');
});

test('ShortcodeTable uses initials of each underscore/hyphen-separated word', () => {
  const codes = new ShortcodeTable();
  assert.equal(codes.codeFor('COUNTER'), 'C');
  assert.equal(codes.codeFor('RELAY'), 'R');
  assert.equal(codes.codeFor('MCU_LINK'), 'ML');
  assert.equal(codes.codeFor('MCU_LINK_RX'), 'MLR');
  assert.equal(codes.codeFor('MCU_LINK_TX'), 'MLT');
  assert.equal(codes.codeFor('NODE-RED'), 'NR');
  assert.equal(codes.codeFor('NODE-RED-COAP'), 'NRC');
});

test('ShortcodeTable falls back to a longer prefix on collision', () => {
  const codes = new ShortcodeTable();
  // Both reduce to "C" as bare initials — second one must not overwrite the first.
  assert.equal(codes.codeFor('COUNTER'), 'C');
  assert.equal(codes.codeFor('CLIENT'), 'CL');
  assert.equal(codes.codeFor('COUNTER'), 'C');
  assert.equal(codes.codeFor('CLIENT'), 'CL');
});

test('estimateTokens is roughly chars/4', () => {
  assert.equal(estimateTokens('abcd'), 1);
  assert.equal(estimateTokens('abcdefgh'), 2);
  assert.equal(estimateTokens(''), 0);
});
