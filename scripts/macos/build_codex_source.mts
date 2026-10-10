// Build the exact reviewed upstream commit with its native package builder.
import { createHash } from 'node:crypto';
import { execFileSync } from 'node:child_process';
import { createReadStream, existsSync, mkdirSync, readFileSync, renameSync, rmSync, writeFileSync } from 'node:fs';
import { join } from 'node:path';

export type SourceLock = {
  version: string;
  upstreamCommit: string;
  source: { url: string; sha256: string };
};

export async function digest(path: string): Promise<string> {
  const hash = createHash('sha256');
  for await (const chunk of createReadStream(path)) hash.update(chunk);
  return hash.digest('hex');
}

export function validateSource(lock: SourceLock): void {
  if (!/^[0-9a-f]{40}$/.test(lock.upstreamCommit)
      || !/^[0-9a-f]{64}$/.test(lock.source.sha256)
      || lock.source.url !== `https://codeload.github.com/openai/codex/tar.gz/${lock.upstreamCommit}`) {
    throw new Error('Expected an exact official Codex source commit and archive digest.');
  }
}

export async function buildSource(root: string, lock: SourceLock, runtime: string, identity: string): Promise<void> {
  validateSource(lock);
  if (!identity || identity === '-') throw new Error('Source runtime requires the app signing identity.');
  const cache = join(root, 'target/codex-source', lock.upstreamCommit);
  mkdirSync(cache, { recursive: true });
  const archive = join(cache, 'source.tar.gz');
  if (!existsSync(archive)) {
    const partial = `${archive}.partial`;
    try {
      execFileSync('curl', ['--fail', '--location', '--retry', '2', '--output', partial, lock.source.url], { stdio: 'inherit' });
      if (await digest(partial) !== lock.source.sha256) throw new Error('Codex source archive digest mismatch.');
      renameSync(partial, archive);
    } finally { rmSync(partial, { force: true }); }
  }
  if (await digest(archive) !== lock.source.sha256) throw new Error('Cached Codex source archive digest mismatch.');
  const source = join(cache, 'source');
  // Recreate only this script's source cache; never trust edits left in an extracted tree.
  rmSync(source, { recursive: true, force: true });
  mkdirSync(source);
  execFileSync('tar', ['-xzf', archive, '--strip-components=1', '-C', source]);
  const cargo = join(cache, 'cargo-stable-locked');
  writeFileSync(cargo, '#!/bin/sh\nexec cargo +stable "$@" --locked\n', { mode: 0o700 });
  const packageDir = join(cache, 'package');
  const lockPath = join(source, 'codex-rs/Cargo.lock');
  const cargoLockDigest = await digest(lockPath);
  execFileSync('python3', [join(source, 'scripts/build_codex_package.py'),
    '--target', 'aarch64-apple-darwin', '--cargo', cargo, '--cargo-profile', 'release',
    '--package-dir', packageDir, '--force'], {
    cwd: source,
    env: { ...process.env, CODEX_REPO_ROOT: source, CARGO_TARGET_DIR: join(cache, 'build'),
      STABLE_GIT_COMMIT: lock.upstreamCommit, GIT_CEILING_DIRECTORIES: cache },
    stdio: 'inherit',
  });
  if (await digest(lockPath) !== cargoLockDigest) throw new Error('The source build changed the upstream Cargo lock.');
  const metadata = JSON.parse(readFileSync(join(packageDir, 'codex-package.json'), 'utf8'));
  if (metadata.version !== lock.version || metadata.target !== 'aarch64-apple-darwin'
      || metadata.variant !== 'codex' || metadata.layoutVersion !== 1) {
    throw new Error('Built Codex package identity mismatch.');
  }
  mkdirSync(runtime, { recursive: true });
  execFileSync('ditto', [packageDir, runtime]);
  const app = join(runtime, 'CodexCLI.app');
  const contents = join(app, 'Contents');
  mkdirSync(join(contents, 'MacOS'), { recursive: true });
  renameSync(join(runtime, 'bin/codex'), join(contents, 'MacOS/codex'));
  writeFileSync(join(contents, 'Info.plist'), `<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0"><dict>
<key>CFBundleIdentifier</key><string>box.acg.decodex.codex-runtime</string>
<key>CFBundleExecutable</key><string>codex</string>
<key>CFBundlePackageType</key><string>APPL</string>
<key>CFBundleVersion</key><string>${lock.version}</string>
</dict></plist>\n`);
  // Use the upstream launcher contract so existing native-image validation still applies.
  const provisioned = readFileSync(join(source, '.github/scripts/macos-signing/provisioned_macos_cli_package.py'), 'utf8');
  const launcher = provisioned.match(/LAUNCHER = """([\s\S]*?)"""/)?.[1];
  if (!launcher) throw new Error('Upstream package launcher contract is missing.');
  writeFileSync(join(runtime, 'bin/codex'), launcher, { mode: 0o755 });
  const signing = join(source, '.github/scripts/macos-signing');
  for (const [path, id, entitlements] of [
    [join(runtime, 'bin/codex-code-mode-host'), 'box.acg.decodex.codex-code-mode-host', join(signing, 'codex-code-mode-host.entitlements.plist')],
    [join(runtime, 'codex-path/rg'), 'box.acg.decodex.rg', ''],
    [app, 'box.acg.decodex.codex-runtime', join(signing, 'codex.entitlements.plist')],
  ]) {
    execFileSync('codesign', ['--force', '--options', 'runtime', '--timestamp=none', '--sign', identity,
      '--identifier', id, ...(entitlements ? ['--entitlements', entitlements] : []), path], { stdio: 'inherit' });
    execFileSync('codesign', ['--verify', '--strict', path], { stdio: 'inherit' });
  }
}
