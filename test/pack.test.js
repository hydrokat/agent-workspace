import { describe, it, before } from 'node:test';
import assert from 'node:assert/strict';
import { execFileSync } from 'node:child_process';
import { dirname, join } from 'node:path';
import { fileURLToPath } from 'node:url';

const ROOT = dirname(dirname(fileURLToPath(import.meta.url)));

describe('npm pack contents', () => {
  let files;
  before(() => {
    const out = execFileSync('npm', ['pack', '--dry-run', '--json'], { cwd: ROOT, encoding: 'utf8' });
    files = JSON.parse(out)[0].files.map((f) => f.path);
  });

  it('still ships the CLI, sources and templates', () => {
    for (const p of ['bin/agent-workspace.js', 'src/cli.js', 'templates/AGENTS.md',
      'templates/skills/agent-coordination/SKILL.md', 'templates/skills/agent-coordination/scripts/coord.py']) {
      assert.ok(files.includes(p), `${p} packed`);
    }
  });

  it('ships run.sh and the tandem project needed to run it', () => {
    for (const p of ['run.sh', 'tandem/pyproject.toml', 'tandem/uv.lock', 'tandem/min-versions.env',
      'tandem/src/tandem/cli.py', 'tandem/README.md']) {
      assert.ok(files.includes(p), `${p} packed`);
    }
  });

  it('does not ship virtualenvs, caches, or tandem tests', () => {
    const bad = files.filter((f) => /(^|\/)(\.venv|__pycache__|\.pytest_cache|\.mypy_cache)(\/|$)/.test(f) || f.startsWith('tandem/tests/'));
    assert.deepEqual(bad, []);
  });

  it('marks run.sh executable in the tarball', () => {
    const out = execFileSync('npm', ['pack', '--dry-run', '--json'], { cwd: ROOT, encoding: 'utf8' });
    const entry = JSON.parse(out)[0].files.find((f) => f.path === 'run.sh');
    assert.ok((entry.mode & 0o111) !== 0, 'run.sh executable');
  });

  it('keeps tandem out of the Node CLI (init/update/doctor do not mention it)', async () => {
    const { readFileSync } = await import('node:fs');
    for (const f of ['init', 'update', 'doctor']) {
      assert.ok(!/tandem/i.test(readFileSync(join(ROOT, `src/commands/${f}.js`), 'utf8')), f);
    }
  });
});
