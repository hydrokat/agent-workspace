import { mkdirSync, mkdtempSync, rmSync, writeFileSync, readFileSync, existsSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { join, dirname } from 'node:path';
import { spawn } from 'node:child_process';
import { fileURLToPath } from 'node:url';

const ROOT = dirname(dirname(fileURLToPath(import.meta.url)));
const BIN = join(ROOT, 'bin', 'agent-workspace.js');

export function makeTmp() {
  return mkdtempSync(join(tmpdir(), 'aw-test-'));
}

export function cleanup(dir) {
  rmSync(dir, { recursive: true, force: true });
}

export function runCli(args, cwd) {
  return new Promise((resolve) => {
    const proc = spawn(process.execPath, [BIN, ...args], { cwd, env: process.env });
    let stdout = '';
    let stderr = '';
    proc.stdout.on('data', d => { stdout += d; });
    proc.stderr.on('data', d => { stderr += d; });
    proc.on('close', code => resolve({ code, stdout, stderr }));
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
