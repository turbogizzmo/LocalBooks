// Run with `npm test` (Node's built-in runner).
import { test } from 'node:test';
import assert from 'node:assert/strict';
import { contentMatches } from './inboxPreview.js';

const bytes = (text) => new TextEncoder().encode(text);

test('a PDF previews only when its bytes are a PDF', () => {
  assert.equal(contentMatches('application/pdf', bytes('%PDF-1.7\n...')), true);
  assert.equal(contentMatches('application/pdf', bytes('<html><script>alert(1)</script>')), false);
  assert.equal(contentMatches('application/pdf', new Uint8Array()), false);
});

test('images are checked by their signatures', () => {
  assert.equal(contentMatches('image/png', new Uint8Array([0x89, 0x50, 0x4e, 0x47, 0x0d])), true);
  assert.equal(contentMatches('image/jpeg', new Uint8Array([0xff, 0xd8, 0xff, 0xe0])), true);
  assert.equal(contentMatches('image/png', bytes('GIF89a')), false);
  assert.equal(contentMatches('text/html', bytes('%PDF-')), false);
});
