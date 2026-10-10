// Stage the exact reviewed runtime and bind its artifact digest to the service build.
import { execFileSync } from 'node:child_process';
import { existsSync, mkdirSync, mkdtempSync, readFileSync, renameSync, rmSync, writeFileSync } from 'node:fs';
import { join, resolve } from 'node:path';
import { tmpdir } from 'node:os';
import { fileURLToPath } from 'node:url';
import { buildSource, digest, type SourceLock } from './build_codex_source.mts';

type Target = { url?: string; sha256?: string; executableSha256?: string; schemaSha256: string };
type Lock = { version: string; upstreamCommit: string; source?: SourceLock['source']; targets: Record<string, Target> };
const root = resolve(fileURLToPath(new URL('../..', import.meta.url)));
const contents = process.argv[2];
if (!contents || process.platform !== 'darwin' || process.arch !== 'arm64') {
  throw new Error('Expected Decodex.app/Contents on Apple Silicon macOS.');
}
const lock: Lock = JSON.parse(readFileSync(join(root, 'codex-runtime.lock.json'), 'utf8'));
const target = lock.targets['aarch64-apple-darwin'];
const runtime = join(contents, 'Resources/CodexRuntime');
const schemas = join(contents, 'Resources/CodexSchema');
if (lock.source) {
  await buildSource(root, { ...lock, source: lock.source }, runtime, process.argv[3]);
} else {
  if (!target.url || !target.sha256 || !target.executableSha256) throw new Error('Incomplete official runtime lock.');
  const cache = join(root, 'target/codex-bundle-cache');
  mkdirSync(cache, { recursive: true });
  const archive = join(cache, `codex-provisioned-package-aarch64-apple-darwin-${lock.version}.tar.gz`);
  if (!existsSync(archive)) {
    const partial = `${archive}.partial`;
    try {
      execFileSync('curl', ['--fail', '--location', '--retry', '2', '--output', partial, target.url], { stdio: 'inherit' });
      if (await digest(partial) !== target.sha256) throw new Error('Codex download checksum differs from the release lock.');
      renameSync(partial, archive);
    } finally { rmSync(partial, { force: true }); }
  }
  if (await digest(archive) !== target.sha256) throw new Error('Cached Codex archive differs from the release lock.');
  const scratch = mkdtempSync(join(tmpdir(), 'decodex-codex-bundle-'));
  try {
    execFileSync('tar', ['-xzf', archive, '-C', scratch]);
    execFileSync('ditto', [scratch, runtime]);
  } finally { rmSync(scratch, { recursive: true, force: true }); }
}
const executable = join(runtime, 'CodexCLI.app/Contents/MacOS/codex');
const executableSha256 = await digest(executable);
if (target.executableSha256 && executableSha256 !== target.executableSha256) throw new Error('Codex executable differs from the release lock.');
execFileSync('codesign', ['--verify', '--deep', '--strict', join(runtime, 'CodexCLI.app')]);
const version = execFileSync(executable, ['--version'], { encoding: 'utf8' }).trim();
if (version !== `codex-cli ${lock.version}`) throw new Error(`Unexpected Codex version: ${version}`);
rmSync(schemas, { recursive: true, force: true });
execFileSync(executable, ['app-server', 'generate-json-schema', '--experimental', '--out', schemas]);
const fingerprint = execFileSync('cargo', ['+stable', 'run', '--locked', '--quiet', '--release', '-p', 'decodex-codex', '--example', 'schema_fingerprint', '--', schemas], { cwd: root, encoding: 'utf8' }).trim();
if (fingerprint !== target.schemaSha256) throw new Error('Codex protocol differs from the release lock.');
writeFileSync(join(runtime, 'decodex-runtime-evidence.json'), JSON.stringify({ upstreamCommit: lock.upstreamCommit, executableSha256, schemaSha256: fingerprint }) + '\n');
process.stdout.write(`Bundled Codex ${lock.version} (${lock.upstreamCommit})\n`);
