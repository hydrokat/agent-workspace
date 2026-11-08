import { mkdirSync, mkdtempSync, rmSync, writeFileSync, readFileSync, existsSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { join, dirname } from 'node:path';
import { spawn, spawnSync } from 'node:child_process';
import { fileURLToPath } from 'node:url';

const ROOT = dirname(dirname(fileURLToPath(import.meta.url)));
const BIN = join(ROOT, 'bin', 'agent-workspace.js');

export function makeTmp() {
  return mkdtempSync(join(tmpdir(), 'aw-test-'));
}

export function cleanup(dir) {
  rmSync(dir, { recursive: true, force: true });
}

export function runRunSh(args = [], { env = {}, script = join(ROOT, 'run.sh'), skipUpdate = true } = {}) {
  const cwd = mkdtempSync(join(tmpdir(), 'aw-runsh-'));
  const merged = { ...process.env, NO_COLOR: '1' };
  delete merged.TANDEM_SKIP_UPDATE;
  delete merged.TANDEM_AUTO_UPDATE;
  if (skipUpdate) merged.TANDEM_SKIP_UPDATE = '1';
  for (const [k, v] of Object.entries(env)) {
    if (v === undefined) delete merged[k];
    else merged[k] = v;
  }
  try {
    const r = spawnSync('/bin/bash', [script, ...args], { cwd, env: merged, encoding: 'utf8' });
    return { code: r.status ?? 1, output: (r.stdout || '') + (r.stderr || ''), cwd };
  } finally {
    rmSync(cwd, { recursive: true, force: true });
  }
}

export function runCli(args, cwd, env = process.env) {
  return new Promise((resolve) => {
    const proc = spawn(process.execPath, [BIN, ...args], { cwd, env });
    let stdout = '';
    let stderr = '';
    proc.stdout.on('data', d => { stdout += d; });
    proc.stderr.on('data', d => { stderr += d; });
    proc.on('close', code => resolve({ code, stdout, stderr }));
  });
}

export function runCliWithInput(args, cwd, input, env = process.env) {
  return new Promise((resolve) => {
    const proc = spawn(process.execPath, [BIN, ...args], { cwd, env });
    let stdout = '';
    let stderr = '';
    proc.stdout.on('data', d => { stdout += d; });
    proc.stderr.on('data', d => { stderr += d; });
    proc.on('close', code => resolve({ code, stdout, stderr }));
    proc.stdin.end(input);
  });
}

export function fileExists(dir, rel) {
  return existsSync(join(dir, rel));
}

export function readFile(dir, rel) {
  return readFileSync(join(dir, rel), 'utf8');
}

export function writeFile(dir, rel, content) {
  const full = join(dir, rel);
  mkdirSync(dirname(full), { recursive: true });
  writeFileSync(full, content, 'utf8');
}
