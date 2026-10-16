import { readdirSync, readFileSync, chmodSync, statSync } from 'node:fs';
import { join } from 'node:path';
import { templatesDir } from './assets.js';
import { writeManaged } from './manifest.js';
import { log } from './log.js';

const PACKAGE_ROOT = join(templatesDir(), '..');
const TANDEM_DEST = '.agent-workspace/tandem';
const TANDEM_FILES = ['pyproject.toml', 'uv.lock', 'min-versions.env', 'README.md'];

function walk(dir, base = '') {
  const out = [];
  for (const name of readdirSync(join(dir, base)).sort()) {
    if (name === '__pycache__') continue;
    const rel = base ? `${base}/${name}` : name;
    if (statSync(join(dir, rel)).isDirectory()) out.push(...walk(dir, rel));
    else out.push(rel);
  }
  return out;
}

function runtimeFiles() {
  const files = [{ from: 'run.sh', to: 'run.sh', exec: true }];
  for (const f of TANDEM_FILES) files.push({ from: `tandem/${f}`, to: `${TANDEM_DEST}/${f}` });
  for (const f of walk(join(PACKAGE_ROOT, 'tandem', 'src'))) {
    files.push({ from: `tandem/src/${f}`, to: `${TANDEM_DEST}/src/${f}` });
  }
  return files;
}

/** Install `run.sh` at the workspace root and the tandem project under `.agent-workspace/tandem/`. */
export function installRuntime(target, manifest, dryRun = false) {
  log.step('Installing run.sh and tandem');
  for (const { from, to, exec } of runtimeFiles()) {
    const action = writeManaged(target, to, readFileSync(join(PACKAGE_ROOT, from), 'utf8'), manifest, dryRun);
    if (exec && !dryRun) chmodSync(join(target, to), 0o755);
    if (action === 'created') log.created(to);
    else if (action === 'updated') log.updated(to);
    else log.unchanged(to);
  }
}
