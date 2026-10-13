// Explicit developer preparation; no Codex build runs during app packaging or startup.
import { execFileSync, spawnSync } from 'node:child_process';
import { readFileSync } from 'node:fs';
import { join, resolve } from 'node:path';
import { fileURLToPath } from 'node:url';
import { buildSource, type SourceLock } from './build_codex_source.mts';

const root = resolve(fileURLToPath(new URL('../..', import.meta.url)));
const lockPath = join(root, 'codex-runtime.lock.json');
const command = ['+stable', 'run', '--locked', '--quiet', '-p', 'decodex-components', '--'];
const supplied = process.env.DECODEX_CODEX_DEVELOPMENT_RUNTIME;
if (!supplied && spawnSync('cargo', [...command, 'prepare', lockPath], { cwd: root, stdio: 'inherit' }).status === 0) process.exit(0);
const lock: SourceLock = JSON.parse(readFileSync(lockPath, 'utf8'));
if (!lock.source) throw new Error('The official component could not be downloaded. Retry cargo make prepare-codex.');
const runtime = supplied ? resolve(supplied) : join(root, 'target/codex-development', lock.upstreamCommit, 'runtime');
if (!supplied) await buildSource(root, lock, runtime, process.env.DECODEX_APP_SIGN_IDENTITY || '4EBCADF6B4D513E45CE33EC6934C08DBB0F03D7F');
execFileSync('cargo', [...command, 'import-dev', lockPath, runtime], { cwd: root, stdio: 'inherit' });
