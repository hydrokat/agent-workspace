import { describe, it, before, after } from 'node:test';
import assert from 'node:assert/strict';
import { existsSync, mkdirSync, readdirSync, readFileSync, statSync, utimesSync, writeFileSync } from 'node:fs';
import { join } from 'node:path';
import { execFileSync } from 'node:child_process';
import { makeTmp, cleanup, runCli, fileExists, readFile, writeFile } from './helpers.js';

const INSTALLED = [
  'run.sh',
  '.agent-workspace/tandem/Cargo.toml',
  '.agent-workspace/tandem/Cargo.lock',
  '.agent-workspace/tandem/rust-toolchain.toml',
  '.agent-workspace/tandem/min-versions.env',
  '.agent-workspace/tandem/README.md',
  '.agent-workspace/tandem/src/main.rs',
  '.agent-workspace/tandem/src/lib.rs',
];

function listFiles(dir) {
  const out = [];
  for (const name of readdirSync(dir)) {
    const full = join(dir, name);
    if (statSync(full).isDirectory()) out.push(...listFiles(full).map((f) => `${name}/${f}`));
    else out.push(name);
  }
  return out;
}

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
    assert.ok(!fileExists(dir, '.agent-workspace/tandem/target'), 'no build output');
    assert.ok(!fileExists(dir, '.agent-workspace/tandem/packaging'), 'no packaging scripts');
  });

  it('installs only Rust sources under tandem/src and no Python', () => {
    const files = listFiles(join(dir, '.agent-workspace/tandem'));
    assert.deepEqual(files.filter((f) => /\.py$|pyproject\.toml$|uv\.lock$|__pycache__/.test(f)), []);
    assert.ok(files.filter((f) => f.startsWith('src/')).every((f) => f.endsWith('.rs')), 'only .rs under src');
  });

  it('installs a Cargo.toml version that run.sh --check can read', () => {
    let out = '';
    try {
      out = execFileSync(join(dir, 'run.sh'), ['--check', '--no-update'], { cwd: dir, encoding: 'utf8', stdio: ['ignore', 'pipe', 'pipe'] });
    } catch (e) {
      out = `${e.stdout}${e.stderr}`;
    }
    assert.match(out, /runtime: (binary|source)/);
    assert.ok(!/unknown version/.test(out), out);
  });

  it('installed run.sh finds its tandem project (--check reads min-versions)', () => {
    let out = '';
    try {
      out = execFileSync(join(dir, 'run.sh'), ['--check', '--no-update'], { cwd: dir, encoding: 'utf8', stdio: ['ignore', 'pipe', 'pipe'] });
    } catch (e) {
      out = `${e.stdout}${e.stderr}`;
    }
    assert.ok(!/Missing min-versions/.test(out), out);
    assert.match(out, /tandem bootstrap/);
  });

  it('update restores a modified run.sh (with .bak) and reinstalls a deleted tandem file', async () => {
    writeFile(dir, 'run.sh', '#!/bin/sh\necho hacked\n');
    const { unlinkSync } = await import('node:fs');
    unlinkSync(join(dir, '.agent-workspace/tandem/Cargo.toml'));
    const { code } = await runCli(['update', '--yes', '--no-link'], dir);
    assert.equal(code, 0);
    assert.ok(!readFile(dir, 'run.sh').includes('hacked'));
    assert.ok(fileExists(dir, 'run.sh.bak'));
    assert.ok((statSync(join(dir, 'run.sh')).mode & 0o111) !== 0, 'still executable');
    assert.ok(fileExists(dir, '.agent-workspace/tandem/Cargo.toml'));
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
    assert.ok(gitignore.split('\n').some((l) => l.trim() === 'test-runs/'), 'test-runs/ in .gitignore');
  });
  describe('legacy Python tandem cleanup on update', () => {
    const TANDEM = '.agent-workspace/tandem';
    const LEGACY = [
      `${TANDEM}/pyproject.toml`,
      `${TANDEM}/uv.lock`,
      `${TANDEM}/src/tandem/__init__.py`,
      `${TANDEM}/src/tandem/cli.py`,
      `${TANDEM}/src/tandem/adapters/base.py`,
    ];
    const MODIFIED = `${TANDEM}/src/tandem/cli.py`;
    const LAST_UPDATE = new Date('2026-09-01T00:00:00Z');

    function oldWorkspace() {
      const old = makeTmp();
      return runCli(['init', '--yes', '--providers', 'claude', '--no-link'], old).then(() => {
        const manifestFile = join(old, '.agent-workspace/manifest.json');
        const manifest = JSON.parse(readFileSync(manifestFile, 'utf8'));
        manifest.updatedAt = LAST_UPDATE.toISOString();
        for (const rel of LEGACY) {
          writeFile(old, rel, `# legacy ${rel}\n`);
          utimesSync(join(old, rel), LAST_UPDATE, LAST_UPDATE);
          manifest.managedFiles.push(rel);
        }
        writeFile(old, MODIFIED, '# legacy but edited by the user\n');
        const edited = new Date(LAST_UPDATE.getTime() + 3600_000);
        utimesSync(join(old, MODIFIED), edited, edited);
        mkdirSync(join(old, TANDEM, 'src/tandem/__pycache__'), { recursive: true });
        writeFileSync(join(old, TANDEM, 'src/tandem/__pycache__/cli.cpython-313.pyc'), 'x');
        writeFileSync(manifestFile, JSON.stringify(manifest, null, 2));
        return old;
      });
    }

    it('removes managed legacy Python files and drops them from the manifest', async () => {
      const old = await oldWorkspace();
      try {
        const { code } = await runCli(['update', '--yes', '--no-link'], old);
        assert.equal(code, 0);
        for (const rel of LEGACY) assert.ok(!fileExists(old, rel), `${rel} removed`);
        assert.deepEqual(listFiles(join(old, TANDEM, 'src/tandem')), ['cli.py.bak'], 'only the backup of the edited file remains (no adapters/, no __pycache__)');
        const manifest = JSON.parse(readFileSync(join(old, '.agent-workspace/manifest.json'), 'utf8'));
        assert.deepEqual(manifest.managedFiles.filter((f) => LEGACY.includes(f)), []);
        assert.ok(manifest.managedFiles.includes(`${TANDEM}/Cargo.toml`));
        assert.ok(fileExists(old, `${TANDEM}/src/main.rs`));
      } finally { cleanup(old); }
    });

    it('keeps a .bak only for legacy files modified since the last update', async () => {
      const old = await oldWorkspace();
      try {
        await runCli(['update', '--yes', '--no-link'], old);
        assert.ok(fileExists(old, `${MODIFIED}.bak`), 'modified file backed up');
        assert.match(readFile(old, `${MODIFIED}.bak`), /edited by the user/);
        for (const rel of LEGACY.filter((f) => f !== MODIFIED)) assert.ok(!fileExists(old, `${rel}.bak`), `${rel} not backed up`);
      } finally { cleanup(old); }
    });

    it('leaves unmanaged Python files in the legacy directory alone', async () => {
      const old = await oldWorkspace();
      try {
        writeFile(old, `${TANDEM}/src/tandem/mine.py`, 'print("mine")\n');
        await runCli(['update', '--yes', '--no-link'], old);
        assert.ok(fileExists(old, `${TANDEM}/src/tandem/mine.py`), 'user file kept');
        assert.ok(!fileExists(old, `${TANDEM}/src/tandem/cli.py`));
      } finally { cleanup(old); }
    });

    it('--dry-run removes nothing', async () => {
      const old = await oldWorkspace();
      try {
        await runCli(['update', '--yes', '--no-link', '--dry-run'], old);
        for (const rel of LEGACY) assert.ok(existsSync(join(old, rel)), `${rel} still there`);
      } finally { cleanup(old); }
    });
  });
});
