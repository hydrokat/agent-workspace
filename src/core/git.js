import { spawnSync } from 'node:child_process';
import { existsSync, realpathSync } from 'node:fs';
import { join } from 'node:path';

const PROTECTED = ['main', 'master', 'trunk'];

function git(cwd, args, input) {
  const r = spawnSync('git', args, { cwd, encoding: 'utf8', input, stdio: ['pipe', 'pipe', 'pipe'] });
  return { ok: r.status === 0, out: r.stdout ?? '', err: (r.stderr ?? '').trim() };
}

export function gitAvailable() {
  return spawnSync('git', ['--version'], { stdio: 'ignore' }).status === 0;
}

/** 'own' when `cwd` is the top level of a repository, 'nested' when inside another repo, else 'none'. */
export function repoState(cwd) {
  if (!gitAvailable()) return 'none';
  const top = git(cwd, ['rev-parse', '--show-toplevel']);
  if (!top.ok) return 'none';
  return realpathSync(top.out.trim()) === realpathSync(cwd) ? 'own' : 'nested';
}

export function currentBranch(cwd) {
  const r = git(cwd, ['branch', '--show-current']);
  return r.ok ? r.out.trim() : '';
}

export function isProtectedBranch(name) {
  return PROTECTED.includes(name);
}

export function initRepo(cwd) {
  const r = git(cwd, ['init', '-q']);
  if (!r.ok) throw new Error(`git init failed: ${r.err}`);
}

function statusPaths(cwd) {
  const r = git(cwd, ['status', '--porcelain', '-z', '-uall']);
  if (!r.ok) return [];
  const parts = r.out.split('\0').filter(Boolean);
  const paths = [];
  for (let i = 0; i < parts.length; i++) {
    const entry = parts[i];
    const code = entry.slice(0, 2);
    paths.push(entry.slice(3));
    if (code[0] === 'R' || code[0] === 'C') i++;
  }
  return paths;
}

/** path -> content hash (or 'deleted') for every path git reports as changed right now. */
export function snapshot(cwd) {
  const paths = statusPaths(cwd);
  const snap = new Map();
  if (paths.length === 0) return snap;
  const existing = [];
  for (const p of paths) {
    if (existsSync(join(cwd, p))) existing.push(p); else snap.set(p, 'deleted');
  }
  if (existing.length) {
    const r = git(cwd, ['hash-object', '--stdin-paths'], existing.join('\n') + '\n');
    const hashes = r.out.split('\n').filter(Boolean);
    existing.forEach((p, i) => snap.set(p, hashes[i] ?? 'unknown'));
  }
  return snap;
}

/** Paths that changed (or appeared) between two snapshots; `.bak` backups are never included. */
export function changedBetween(before, after) {
  const changed = [];
  for (const [path, hash] of after) {
    if (path.endsWith('.bak')) continue;
    if (before.get(path) !== hash) changed.push(path);
  }
  for (const path of before.keys()) {
    if (!after.has(path) && !path.endsWith('.bak')) changed.push(path);
  }
  return changed.sort();
}

/** Commit exactly `paths`; anything else the user has staged or modified is left alone. */
export function commitPaths(cwd, paths, message) {
  const add = git(cwd, ['add', '-A', '--', ...paths]);
  if (!add.ok) throw new Error(`git add failed: ${add.err}`);
  const commit = git(cwd, ['commit', '-q', '-m', message, '--', ...paths]);
  if (!commit.ok) throw new Error(`git commit failed: ${commit.err}`);
}
