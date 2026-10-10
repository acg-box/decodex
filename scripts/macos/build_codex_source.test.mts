import assert from 'node:assert/strict';
import test from 'node:test';
import { validateSource } from './build_codex_source.mts';

test('source builds require the exact official commit archive', () => {
  const upstreamCommit = 'a'.repeat(40);
  const lock = { version: '0.0.0', upstreamCommit, source: {
    url: `https://codeload.github.com/openai/codex/tar.gz/${upstreamCommit}`, sha256: 'b'.repeat(64),
  } };
  assert.doesNotThrow(() => validateSource(lock));
  for (const source of [
    { ...lock.source, url: lock.source.url.replace(upstreamCommit, 'main') },
    { ...lock.source, url: lock.source.url.replace('openai', 'another-owner') },
    { ...lock.source, sha256: 'missing' },
  ]) assert.throws(() => validateSource({ ...lock, source }));
  assert.throws(() => validateSource({ ...lock, upstreamCommit: '../elsewhere' }));
});
