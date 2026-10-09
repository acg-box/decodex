// Stage the exact official runtime and protocol evidence locked for this Decodex release.
import { createHash } from 'node:crypto';
import { execFileSync } from 'node:child_process';
import { createReadStream, existsSync, mkdirSync, mkdtempSync, readFileSync, rmSync } from 'node:fs';
import { join, resolve } from 'node:path';
import { tmpdir } from 'node:os';
import { fileURLToPath } from 'node:url';

type Target = { url: string; sha256: string; executableSha256: string; schemaSha256: string };
type Lock = { version: string; tag: string; upstreamCommit: string; targets: Record<string, Target> };
const root = resolve(fileURLToPath(new URL('../..', import.meta.url)));
const contents = process.argv[2];
if (!contents || process.platform !== 'darwin' || process.arch !== 'arm64') {
  throw new Error('Expected Decodex.app/Contents on Apple Silicon macOS.');
}
const lock: Lock = JSON.parse(readFileSync(join(root, 'codex-runtime.lock.json'), 'utf8'));
const target = lock.targets['aarch64-apple-darwin'];
const cache = join(root, 'target/codex-bundle-cache');
mkdirSync(cache, { recursive: true });
const archive = join(cache, `codex-provisioned-package-aarch64-apple-darwin-${lock.version}.tar.gz`);
async function sha256(path: string): Promise<string> {
  const hash = createHash('sha256');
  for await (const chunk of createReadStream(path)) hash.update(chunk);
  return hash.digest('hex');
}
if (!existsSync(archive)) {
  const partial = `${archive}.partial`;
  try {
    execFileSync('curl', ['--fail', '--location', '--retry', '2', '--output', partial, target.url], { stdio: 'inherit' });
    if (await sha256(partial) !== target.sha256) throw new Error('Codex download checksum differs from the release lock.');
    const { renameSync } = await import('node:fs');
    renameSync(partial, archive);
  } finally { rmSync(partial, { force: true }); }
}
if (await sha256(archive) !== target.sha256) throw new Error('Cached Codex archive differs from the release lock.');
const scratch = mkdtempSync(join(tmpdir(), 'decodex-codex-bundle-'));
try {
  execFileSync('tar', ['-xzf', archive, '-C', scratch]);
  const executable = join(scratch, 'CodexCLI.app/Contents/MacOS/codex');
  if (await sha256(executable) !== target.executableSha256) throw new Error('Codex executable differs from the release lock.');
  execFileSync('codesign', ['--verify', '--deep', '--strict', join(scratch, 'CodexCLI.app')]);
  const version = execFileSync(executable, ['--version'], { encoding: 'utf8' }).trim();
  if (version !== `codex-cli ${lock.version}`) throw new Error(`Unexpected Codex version: ${version}`);
  const runtime = join(contents, 'Resources/CodexRuntime');
  execFileSync('ditto', [scratch, runtime]);
  const schemas = join(contents, 'Resources/CodexSchema');
  execFileSync(executable, ['app-server', 'generate-json-schema', '--experimental', '--out', schemas]);
  const fingerprint = execFileSync('cargo', ['+stable', 'run', '--locked', '--quiet', '--release', '-p', 'decodex-codex', '--example', 'schema_fingerprint', '--', schemas], { cwd: root, encoding: 'utf8' }).trim();
  if (fingerprint !== target.schemaSha256) throw new Error('Codex protocol differs from the release lock.');
  process.stdout.write(`Bundled Codex ${lock.version} (${lock.upstreamCommit})\n`);
} finally { rmSync(scratch, { recursive: true, force: true }); }
