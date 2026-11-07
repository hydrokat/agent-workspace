import { readdirSync, readFileSync, writeFileSync, existsSync, chmodSync, statSync, rmSync, rmdirSync } from 'node:fs';
import { join } from 'node:path';
import { templatesDir } from './assets.js';
import { writeManaged } from './manifest.js';
import { log } from './log.js';

const PACKAGE_ROOT = join(templatesDir(), '..');
const TANDEM_DEST = '.agent-workspace/tandem';
const TANDEM_FILES = ['Cargo.toml', 'Cargo.lock', 'rust-toolchain.toml', 'min-versions.env', 'README.md'];
const WALK_SKIP = new Set(['target', 'tests']);
const MODIFIED_GRACE_MS = 2000;
const GITIGNORE_ENTRIES = ['update-notice.env', 'update-check.env', 'test-runs/'];

function walk(dir, base = '') {
  const out = [];
  for (const name of readdirSync(join(dir, base)).sort()) {
    if (WALK_SKIP.has(name)) continue;
    const rel = base ? `${base}/${name}` : name;
    if (statSync(join(dir, rel)).isDirectory()) out.push(...walk(dir, rel));
    else if (name.endsWith('.rs')) out.push(rel);
  }
  return out;
}

function runtimeFiles() {
  const files = [{ from: 'run.sh', to: 'run.sh', exec: true }];
  for (const f of TANDEM_FILES) files.push({ from: `tandem/${f}`, to: `${TANDEM_DEST}/${f}` });
  for (const f of walk(join(PACKAGE_ROOT, 'tandem', 'src'))) {
    files.push({ from: `tandem/src/${f}`, to: `${TANDEM_DEST}/src/${f}` });
  }
  if (existsSync(join(PACKAGE_ROOT, 'CHANGELOG.md'))) {
    files.push({ from: 'CHANGELOG.md', to: `${TANDEM_DEST}/CHANGELOG.md` });
  }
  return files;
}

function ensureGitignoreEntries(target, dryRun) {
  const rel = '.agent-workspace/.gitignore';
  const full = join(target, rel);
  if (!existsSync(full)) return;
  const existing = readFileSync(full, 'utf8');
  const lines = existing.split('\n');
  const toAdd = GITIGNORE_ENTRIES.filter((e) => !lines.some((l) => l.trim() === e));
  if (toAdd.length === 0) return;
  if (!dryRun) writeFileSync(full, existing + toAdd.map((e) => `${e}\n`).join(''), 'utf8');
  log.updated(rel);
}

function pruneEmptyRetiredDirs(dir) {
  if (!existsSync(dir)) return;
  for (const name of readdirSync(dir)) {
    const full = join(dir, name);
    if (!statSync(full).isDirectory()) continue;
    if (name.includes('cache')) {
      rmSync(full, { recursive: true, force: true });
    } else {
      pruneEmptyRetiredDirs(full);
    }
  }
  if (readdirSync(dir).length === 0) rmdirSync(dir);
}

function removeRetiredRuntimeFiles(target, manifest, dryRun, currentFiles) {
  const isRetired = (rel) => rel.startsWith(`${TANDEM_DEST}/`) && !currentFiles.has(rel);
  const retired = manifest.managedFiles.filter(isRetired);
  if (retired.length === 0) return;
  const lastUpdate = Date.parse(manifest.updatedAt);
  for (const rel of retired) {
    const full = join(target, rel);
    if (existsSync(full)) {
      const edited = Number.isNaN(lastUpdate) || statSync(full).mtimeMs > lastUpdate + MODIFIED_GRACE_MS;
      if (!dryRun) {
        if (edited) {
          writeFileSync(`${full}.bak`, readFileSync(full));
          log.backedUp(rel);
        }
        rmSync(full);
      }
      log.removed(rel);
    }
  }
  if (dryRun) return;
  manifest.managedFiles = manifest.managedFiles.filter((rel) => !isRetired(rel));
  pruneEmptyRetiredDirs(join(target, TANDEM_DEST, 'src/tandem'));
}

/** Install `run.sh` at the workspace root and the tandem project under `.agent-workspace/tandem/`. */
export function installRuntime(target, manifest, dryRun = false) {
  log.step('Installing run.sh and tandem');
  const files = runtimeFiles();
  const currentFiles = new Set(files.map((f) => f.to));
  for (const { from, to, exec } of files) {
    const action = writeManaged(target, to, readFileSync(join(PACKAGE_ROOT, from), 'utf8'), manifest, dryRun);
    if (exec && !dryRun) chmodSync(join(target, to), 0o755);
    if (action === 'created') log.created(to);
    else if (action === 'updated') log.updated(to);
    else log.unchanged(to);
  }
  removeRetiredRuntimeFiles(target, manifest, dryRun, currentFiles);
  ensureGitignoreEntries(target, dryRun);
}
