import { test } from 'node:test';
import assert from 'node:assert/strict';
import { PLAIN_COPY_NOTE, savedCopyText } from './plainCopy.js';

test('a saved export says where it went and that it is a plain copy', () => {
  assert.equal(savedCopyText('/tmp/P&L.csv'), `Saved /tmp/P&L.csv. ${PLAIN_COPY_NOTE}`);
  assert.equal(savedCopyText('/tmp/a.zip', '3 attachments'), `Saved 3 attachments to /tmp/a.zip. ${PLAIN_COPY_NOTE}`);
});
