import test from 'node:test';
import assert from 'node:assert/strict';
import { decodeClipboardRequest, installClipboardHandler } from '../src/clipboard.js';

const encode = (text) => Buffer.from(text).toString('base64');
test('OSC 52 decodes UTF-8 and accepts clipboard selectors', () => {
  for (const selector of ['c', '', 'p', 's0']) {
    assert.equal(decodeClipboardRequest(`${selector};${encode('Hello 👋\nsecond line')}`), 'Hello 👋\nsecond line');
  }
});
test('queries, invalid data and oversized copies are ignored', () => {
  for (const value of ['c;?', 'c;', 'c;!!!', 'c;A', 'c;/w==', 'bad;SGk=', 'x', `c;${encode('x'.repeat(100001))}`]) {
    assert.equal(decodeClipboardRequest(value), null);
  }
});
test('handler consumes requests, never sends clipboard data to terminal', () => {
  let handler;
  const copies = [];
  const disposable = {dispose() {}};
  const term = {parser: {registerOscHandler(number, callback) {
    assert.equal(number, 52);
    handler = callback;
    return disposable;
  }}};
  assert.equal(installClipboardHandler(term, (text) => copies.push(text)), disposable);
  assert.equal(handler('c;?'), true);
  assert.deepEqual(copies, []);
  assert.equal(handler(`c;${encode('copied')}`), true);
  assert.deepEqual(copies, ['copied']);
});

test('real xterm parser handles split output frames without answering clipboard queries', async () => {
  const { createRequire } = await import('node:module');
  // xterm's browser bundle references self; parsing works without opening a DOM terminal.
  globalThis.self = globalThis;
  const require = createRequire(import.meta.url);
  const { Terminal } = require('@xterm/xterm');
  const term = new Terminal({allowProposedApi: true});
  const copies = [];
  const replies = [];
  const handler = installClipboardHandler(term, (text) => copies.push(text));
  const subscription = term.onData((data) => replies.push(data));
  try {
    for (const chunk of ['\x1b]5', '2;c;SGV', 'sbG8=\x07', '\x1b]52;c;?\x1b\\']) {
      await new Promise((resolve) => term.write(chunk, resolve));
    }
    assert.deepEqual(copies, ['Hello']);
    assert.deepEqual(replies, []);
  } finally {
    subscription.dispose();
    handler.dispose();
    term.dispose();
  }
});
