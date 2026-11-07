import { describe, it, before, after } from 'node:test';
import assert from 'node:assert/strict';
import { execFileSync, spawn } from 'node:child_process';
import { existsSync, mkdirSync, readFileSync, readdirSync, statSync, writeFileSync } from 'node:fs';
import { dirname, join, relative } from 'node:path';
import { fileURLToPath } from 'node:url';
import { cleanup, makeTmp, runCli } from './helpers.js';

const ROOT = dirname(dirname(fileURLToPath(import.meta.url)));

function assertGolden(relPath, actual) {
  const fullPath = join(ROOT, 'test/fixtures/baseline', relPath);
  if (process.env.UPDATE_GOLDEN === '1' || !existsSync(fullPath)) {
    mkdirSync(dirname(fullPath), { recursive: true });
    const content = typeof actual === 'string' ? actual : JSON.stringify(actual, null, 2);
    writeFileSync(fullPath, content, 'utf8');
    return;
  }
  const expectedRaw = readFileSync(fullPath, 'utf8');
  if (relPath.endsWith('.json')) {
    const expected = JSON.parse(expectedRaw);
    assert.deepEqual(actual, expected, `Golden JSON mismatch for ${relPath}`);
  } else {
    assert.equal(actual, expectedRaw, `Golden text mismatch for ${relPath}`);
  }
}

function listFilesRecursive(dir, baseDir = dir) {
  const results = [];
  const entries = readdirSync(dir, { withFileTypes: true });
  for (const entry of entries) {
    const full = join(dir, entry.name);
    if (entry.isDirectory()) {
      results.push(...listFilesRecursive(full, baseDir));
    } else {
      results.push(relative(baseDir, full));
    }
  }
  results.sort();
  return results;
}

describe('characterization baseline tests', () => {
  it('run.sh --help text matches golden snapshot', () => {
    const out = execFileSync('/bin/bash', [join(ROOT, 'run.sh'), '--help'], {
      encoding: 'utf8',
      env: { ...process.env, NO_COLOR: '1' },
    });
    assertGolden('run-sh-help.txt', out);
  });

  it('npm pack contents matches golden snapshot', () => {
    const out = execFileSync('npm', ['pack', '--dry-run', '--json'], {
      cwd: ROOT,
      encoding: 'utf8',
    });
    const files = JSON.parse(out)[0].files.map((f) => f.path);
    files.sort();
    assertGolden('npm-pack-files.json', files);
  });

  it('run.sh scenario exit codes and outputs match golden table', () => {
    const scenarios = [
      { name: 'help-long', args: ['--help'], expectedExit: 0 },
      { name: 'help-short', args: ['-h'], expectedExit: 0 },
      { name: 'check', args: ['--check'], expectedExit: 0 },
      { name: 'answers-missing-value', args: ['--answers'], expectedExit: 4 },
      { name: 'bad-runtime', args: [], env: { TANDEM_RUNTIME: 'invalid_mode_xyz' }, expectedExit: 4 },
    ];

    const results = [];
    for (const s of scenarios) {
      let code = 0;
      let output = '';
      try {
        output = execFileSync('/bin/bash', [join(ROOT, 'run.sh'), ...s.args], {
          encoding: 'utf8',
          env: { ...process.env, NO_COLOR: '1', ...(s.env || {}) },
          stdio: ['ignore', 'pipe', 'pipe'],
        });
      } catch (err) {
        code = err.status ?? 1;
        output = (err.stdout || '') + (err.stderr || '');
      }
      assert.equal(code, s.expectedExit, `Scenario ${s.name} exit code`);
      results.push({
        name: s.name,
        args: s.args,
        exitCode: code,
        hasOutput: output.trim().length > 0,
      });
    }

    assertGolden('run-sh-scenarios.json', results);
  });

  it('init file tree and manifest match golden snapshot', async () => {
    const dir = makeTmp();
    try {
      const { code } = await runCli(['init', '--yes', '--providers', 'claude,gemini,codex', '--no-link'], dir);
      assert.equal(code, 0, 'init should exit 0');

      const files = listFilesRecursive(dir);
      assertGolden('init-file-tree.json', files);

      const manifestRaw = readFileSync(join(dir, '.agent-workspace/manifest.json'), 'utf8');
      const manifest = JSON.parse(manifestRaw);
      assertGolden('manifest.json', {
        version: manifest.version,
        providers: manifest.providers,
        filesCount: manifest.files ? Object.keys(manifest.files).length : 0,
      });
    } finally {
      cleanup(dir);
    }
  });
});
