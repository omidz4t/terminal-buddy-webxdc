import test from 'node:test';
import assert from 'node:assert/strict';
import { Presence, pickUserColor } from '../src/presence.js';

const encode = (message) => new TextEncoder().encode(JSON.stringify(message));

test('three peers converge without answering joins or amplifying frames', async () => {
  const peers = [];
  let sent = 0;
  try {
    for (const addr of ['one', 'two', 'three']) {
      const peer = new Presence(addr, addr, (data) => {
        sent++;
        setTimeout(() => {
          for (const other of peers) other.handleMessage(data);
        }, 1);
      });
      peers.push(peer);
    }
    await new Promise((resolve) => setTimeout(resolve, 100));
    assert.equal(sent, 3);
    for (const peer of peers) assert.equal(peer.list().length, 2);
    for (let i = 0; i < 100; i++) peers[0].announceJoin();
    assert.equal(sent, 3, 'repeated announcements are throttled');
  } finally {
    for (const peer of peers) peer.destroy();
  }
});

test('presence ignores malicious colors and malformed addresses', () => {
  const peer = new Presence('self', 'Self', () => {});
  try {
    peer.handleMessage(encode({t: 'join', a: {}, c: 'bad'}));
    assert.equal(peer.list().length, 0);
    peer.handleMessage(encode({t: 'join', a: 'attacker', c: '\"><img src=x onerror=alert(1)>', cl: 'url(bad)'}));
    assert.equal(peer.list()[0].color, pickUserColor('attacker').color);
    assert.equal(peer.list()[0].colorLight, pickUserColor('attacker').light);
  } finally {
    peer.destroy();
  }
});
