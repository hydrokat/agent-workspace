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

  it('ships the sprint management skills, project-manager agent, and sprints scaffolding', () => {
    const required = [
      'templates/sprints/README.md',
      'templates/agents/project-manager/AGENT.md',
      'templates/knowledgebase/best-practices/sprint-planning-and-governance.md',
      'templates/skills/pm-new-sprint/SKILL.md',
      'templates/skills/pm-new-sprint/agents/openai.yaml',
      'templates/skills/pm-new-sprint/scripts/sprints.py',
      'templates/skills/pm-new-sprint/assets/sprint.md',
      'templates/skills/pm-new-sprint/assets/business-requirements-document.html',
      'templates/skills/pm-new-sprint/assets/functional-requirements-document.html',
      'templates/skills/pm-new-sprint/assets/functional-specification-document.html',
      'templates/skills/pm-sprint-lock/SKILL.md',
      'templates/skills/pm-sprint-lock/agents/openai.yaml',
      'templates/skills/pm-sprint-lock/assets/lock-summary.md',
      'templates/skills/pm-sprint-unlock/SKILL.md',
      'templates/skills/pm-sprint-unlock/agents/openai.yaml',
      'templates/skills/pm-sprint-unlock/assets/unlock-warning.md',
      'templates/skills/pm-sprint-close/SKILL.md',
      'templates/skills/pm-sprint-close/agents/openai.yaml',
      'templates/skills/pm-sprint-close/assets/close-report.md',
      'templates/skills/pm-sprint-implement/SKILL.md',
      'templates/skills/pm-sprint-implement/agents/openai.yaml',
      'templates/skills/pm-sprint-implement/assets/execution-plan.md',
      'templates/skills/pm-sprint-status/SKILL.md',
      'templates/skills/pm-sprint-status/agents/openai.yaml',
      'templates/skills/pm-sprint-status/assets/status.md',
      'templates/skills/pm-sprint-list/SKILL.md',
      'templates/skills/pm-sprint-list/agents/openai.yaml',
      'templates/skills/pm-sprint-list/assets/list.md',
      'templates/skills/pm-commands/SKILL.md',
      'templates/skills/pm-commands/agents/openai.yaml',
      'templates/skills/pm-commands/assets/commands.md',
    ];
    for (const p of required) assert.ok(files.includes(p), `${p} packed`);
  });

  it('ships no bytecode or test fixtures inside the skills', () => {
    assert.deepEqual(files.filter((f) => f.startsWith('templates/') && /(__pycache__|\.pyc$|\.pytest_cache)/.test(f)), []);
  });

  it('ships run.sh and the Rust crate needed to build it from source', () => {
    for (const p of ['run.sh', 'tandem/Cargo.toml', 'tandem/Cargo.lock', 'tandem/rust-toolchain.toml',
      'tandem/min-versions.env', 'tandem/README.md', 'tandem/src/main.rs', 'tandem/src/lib.rs']) {
      assert.ok(files.includes(p), `${p} packed`);
    }
  });

  it('ships CHANGELOG.md at the package root', () => {
    assert.ok(files.includes('CHANGELOG.md'), 'CHANGELOG.md packed');
  });

  it('ships no Python, build output, tests or packaging scripts under tandem/', () => {
    const tandem = files.filter((f) => f.startsWith('tandem/'));
    assert.deepEqual(tandem.filter((f) => /\.py$|(^|\/)(uv\.lock|pyproject\.toml)$/.test(f)), []);
    assert.deepEqual(tandem.filter((f) => /(^|\/)(target|tests|packaging|dist)(\/|$)/.test(f)), []);
    assert.deepEqual(files.filter((f) => /(^|\/)(\.venv|__pycache__|\.pytest_cache|\.mypy_cache)(\/|$)/.test(f) && !f.startsWith('templates/')), []);
  });

  it('ships only .rs files under tandem/src', () => {
    const src = files.filter((f) => f.startsWith('tandem/src/'));
    assert.ok(src.length > 0);
    assert.deepEqual(src.filter((f) => !f.endsWith('.rs')), []);
  });

  it('marks run.sh executable in the tarball', () => {
    const out = execFileSync('npm', ['pack', '--dry-run', '--json'], { cwd: ROOT, encoding: 'utf8' });
    const entry = JSON.parse(out)[0].files.find((f) => f.path === 'run.sh');
    assert.ok((entry.mode & 0o111) !== 0, 'run.sh executable');
  });
});
