import { describe, it, before, after } from 'node:test';
import assert from 'node:assert/strict';
import { statSync } from 'node:fs';
import { join } from 'node:path';
import { execFileSync } from 'node:child_process';
import { makeTmp, cleanup, runCli, fileExists, readFile, writeFile } from './helpers.js';

const INSTALLED = [
  'run.sh',
  '.agent-workspace/tandem/pyproject.toml',
  '.agent-workspace/tandem/uv.lock',
  '.agent-workspace/tandem/min-versions.env',
  '.agent-workspace/tandem/src/tandem/cli.py',
];

describe('run.sh and tandem installation', () => {
  let dir;
  before(async () => {
    dir = makeTmp();
    await runCli(['init', '--yes', '--providers', 'claude', '--no-link'], dir);
  });
  after(() => cleanup(dir));

  it('init installs an executable run.sh and the tandem project', () => {
    for (const rel of INSTALLED) assert.ok(fileExists(dir, rel), `${rel} installed`);
    assert.ok((statSync(join(dir, 'run.sh')).mode & 0o111) !== 0, 'run.sh executable');
    assert.ok(!fileExists(dir, '.agent-workspace/tandem/tests'), 'no tandem tests');
  });

  it('installed run.sh finds its tandem project (--check reads min-versions)', () => {
    let out = '';
    try {
      out = execFileSync(join(dir, 'run.sh'), ['--check'], { cwd: dir, encoding: 'utf8', stdio: ['ignore', 'pipe', 'pipe'] });
    } catch (e) {
      out = `${e.stdout}${e.stderr}`;
    }
    assert.ok(!/Missing min-versions/.test(out), out);
    assert.match(out, /tandem bootstrap/);
  });

  it('update restores a modified run.sh (with .bak) and reinstalls a deleted tandem file', async () => {
    writeFile(dir, 'run.sh', '#!/bin/sh\necho hacked\n');
    const { unlinkSync } = await import('node:fs');
    unlinkSync(join(dir, '.agent-workspace/tandem/pyproject.toml'));
    const { code } = await runCli(['update', '--yes', '--no-link'], dir);
    assert.equal(code, 0);
    assert.ok(!readFile(dir, 'run.sh').includes('hacked'));
    assert.ok(fileExists(dir, 'run.sh.bak'));
    assert.ok((statSync(join(dir, 'run.sh')).mode & 0o111) !== 0, 'still executable');
    assert.ok(fileExists(dir, '.agent-workspace/tandem/pyproject.toml'));
  });

  it('update installs the runtime into a workspace that predates it', async () => {
    const old = makeTmp();
    try {
      await runCli(['init', '--yes', '--providers', 'claude', '--no-link'], old);
      const { rmSync } = await import('node:fs');
      rmSync(join(old, 'run.sh'));
      rmSync(join(old, '.agent-workspace/tandem'), { recursive: true });
      await runCli(['update', '--yes', '--no-link'], old);
      for (const rel of INSTALLED) assert.ok(fileExists(old, rel), rel);
    } finally { cleanup(old); }
  });

  it('--dry-run writes nothing', async () => {
    const d = makeTmp();
    try {
      await runCli(['init', '--yes', '--providers', 'claude', '--no-link', '--dry-run'], d);
      assert.ok(!fileExists(d, 'run.sh'));
    } finally { cleanup(d); }
  });

  it('init installs CHANGELOG.md to .agent-workspace/tandem/CHANGELOG.md', () => {
    assert.ok(fileExists(dir, '.agent-workspace/tandem/CHANGELOG.md'), 'CHANGELOG.md installed');
  });

  it('update adds update-notice.env and update-check.env to .agent-workspace/.gitignore', () => {
    const gitignore = readFile(dir, '.agent-workspace/.gitignore');
    assert.ok(gitignore.includes('update-notice.env'), 'update-notice.env in .gitignore');
    assert.ok(gitignore.includes('update-check.env'), 'update-check.env in .gitignore');
  });
});
