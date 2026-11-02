import assert from 'node:assert/strict';
import { execFileSync } from 'node:child_process';
import { existsSync, mkdirSync, mkdtempSync, readFileSync, rmSync, statSync, utimesSync, writeFileSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
import test from 'node:test';
import { extractSourceArchive, validateSource } from './build_codex_source.mts';

test('source builds require the exact official commit archive', () => {
  const upstreamCommit = 'a'.repeat(40);
  const lock = { version: '0.166.0+decodex.aaaaaaaaaaaa', upstreamCommit, targets: {}, source: {
    url: `https://codeload.github.com/openai/codex/tar.gz/${upstreamCommit}`, sha256: 'b'.repeat(64),
  } };
  assert.doesNotThrow(() => validateSource(lock));
  for (const source of [
    { ...lock.source, url: lock.source.url.replace(upstreamCommit, 'main') },
    { ...lock.source, url: lock.source.url.replace('openai', 'another-owner') },
    { ...lock.source, sha256: 'missing' },
  ]) assert.throws(() => validateSource({ ...lock, source }));
  assert.throws(() => validateSource({ ...lock, upstreamCommit: '../elsewhere' }));
  assert.throws(() => validateSource({ ...lock, version: '0.0.0' }));
});

test('source version preparation regenerates the lock without changing dependencies', () => {
  const source = mkdtempSync(join(tmpdir(), 'decodex-source-version-'));
  try {
    const workspace = join(source, 'codex-rs');
    for (const member of ['core', 'vendor']) mkdirSync(join(workspace, member, 'src'), { recursive: true });
    writeFileSync(join(workspace, 'Cargo.toml'), '[workspace]\nmembers=["core"]\nexclude=["vendor"]\nresolver="2"\n[workspace.package]\nversion="0.0.0"\nedition="2021"\n');
    writeFileSync(join(workspace, 'core/Cargo.toml'), '[package]\nname="codex-core"\nversion.workspace=true\nedition.workspace=true\n[dependencies]\nvendor={path="../vendor"}\n');
    writeFileSync(join(workspace, 'vendor/Cargo.toml'), '[package]\nname="vendor"\nversion="4.2.0"\nedition="2021"\n');
    for (const member of ['core', 'vendor']) writeFileSync(join(workspace, member, 'src/lib.rs'), 'pub fn current() {}\n');
    execFileSync('cargo', ['+stable', 'generate-lockfile', '--offline'], { cwd: workspace });
    const version = '0.166.0+decodex.aaaaaaaaaaaa';
    execFileSync('python3', [new URL('./version_codex_source.py', import.meta.url).pathname, source, version]);
    const metadata = JSON.parse(execFileSync('cargo', ['+stable', 'metadata', '--locked', '--offline', '--no-deps', '--format-version', '1'], { cwd: workspace, encoding: 'utf8' }));
    assert.equal(metadata.packages.find((entry: { name: string }) => entry.name === 'codex-core').version, version);
    assert.match(readFileSync(join(workspace, 'Cargo.lock'), 'utf8'), /name = "vendor"\nversion = "4.2.0"/);
  } finally {
    rmSync(source, { recursive: true, force: true });
  }
});

test('source extraction replaces stale files and refreshes archive timestamps', () => {
  const fixture = mkdtempSync(join(tmpdir(), 'decodex-source-extract-'));
  try {
    const packaged = join(fixture, 'package');
    const source = join(fixture, 'source');
    const archive = join(fixture, 'source.tar.gz');
    mkdirSync(packaged);
    mkdirSync(source);
    writeFileSync(join(packaged, 'lib.rs'), 'pub fn current() {}\n');
    const archiveTime = new Date('2010-01-01T00:00:00Z');
    utimesSync(join(packaged, 'lib.rs'), archiveTime, archiveTime);
    execFileSync('tar', ['-czf', archive, '-C', fixture, 'package']);
    writeFileSync(join(source, 'lib.rs'), 'pub fn stale() {}\n');
    writeFileSync(join(source, 'removed.rs'), 'stale\n');
    const previousBuildTime = Date.now() - 60_000;

    extractSourceArchive(archive, source);

    assert.equal(readFileSync(join(source, 'lib.rs'), 'utf8'), 'pub fn current() {}\n');
    assert.equal(existsSync(join(source, 'removed.rs')), false);
    assert.ok(statSync(join(source, 'lib.rs')).mtimeMs > previousBuildTime,
      'source must be newer than a cached build, regardless of archive timestamps');
  } finally {
    rmSync(fixture, { recursive: true, force: true });
  }
});
