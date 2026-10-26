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

  it('ships the script-first skills with their scripts and templates', () => {
    const skills = 'templates/skills';
    const required = [
      'spec-planner/scripts/specs.py', 'spec-planner/assets/impl.md', 'spec-planner/assets/task.md',
      'spec-planner/assets/delivery-task.md', 'spec-implement/assets/task-metadata.md',
      'spec-compile/assets/completed.md', 'spec-compile/assets/incomplete.md',
      'spec-explain/SKILL.md', 'spec-explain/agents/openai.yaml', 'spec-explain/assets/explanation.md',
      'explain/SKILL.md', 'explain/agents/openai.yaml', 'explain/scripts/locate.py',
      'explain/assets/technical.md', 'explain/assets/non-technical.md',
      'feasibility/SKILL.md', 'feasibility/agents/openai.yaml', 'feasibility/assets/report.md',
      'code-cleanup/SKILL.md', 'code-cleanup/agents/openai.yaml', 'code-cleanup/scripts/comments.py',
      'code-cleanup/assets/candidates.md', 'workspace-cleanup/scripts/scan_workspace.py',
      'workspace-cleanup/assets/candidates.md', 'pull-request/assets/pr-body.md',
      'knowledge/assets/decision.md', 'knowledge/assets/context-note.md', 'knowledge/assets/best-practice.md',
      'knowledge/assets/business-flow.md',
    ];
    for (const rel of required) assert.ok(files.includes(`${skills}/${rel}`), `${rel} packed`);
  });

  it('ships the generic knowledgebase notes for the script-first workflow', () => {
    for (const note of ['deterministic-skill-scripts', 'parallel-spec-execution', 'comment-classification']) {
      assert.ok(files.includes(`templates/knowledgebase/best-practices/${note}.md`), `${note} packed`);
    }
  });

  it('ships no bytecode or test fixtures inside the skills', () => {
    assert.deepEqual(files.filter((f) => f.startsWith('templates/') && /(__pycache__|\.pyc$|\.pytest_cache)/.test(f)), []);
  });

  it('ships run.sh and the tandem project needed to run it', () => {
    for (const p of ['run.sh', 'tandem/pyproject.toml', 'tandem/uv.lock', 'tandem/min-versions.env',
      'tandem/src/tandem/cli.py', 'tandem/README.md']) {
      assert.ok(files.includes(p), `${p} packed`);
    }
  });

  it('ships CHANGELOG.md at the package root', () => {
    assert.ok(files.includes('CHANGELOG.md'), 'CHANGELOG.md packed');
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
});
