import { describe, it, before, after } from 'node:test';
import assert from 'node:assert/strict';
import { execFileSync } from 'node:child_process';
import { makeTmp, cleanup, runCli, fileExists, readFile } from './helpers.js';

let hasPython = true;
try { execFileSync('python3', ['--version'], { stdio: 'ignore' }); } catch { hasPython = false; }

describe('script-first skills ship their scripts and templates', () => {
  let dir;
  before(async () => {
    dir = makeTmp();
    await runCli(['init', '--yes', '--providers', 'claude', '--no-link'], dir);
  });
  after(() => cleanup(dir));

  const skill = (name) => readFile(dir, `.agents/skills/${name}/SKILL.md`);
  const has = (rel) => assert.ok(fileExists(dir, `.agents/skills/${rel}`), rel);

  it('workspace-cleanup calls scan_workspace.py and ships the candidate layout', () => {
    has('workspace-cleanup/scripts/scan_workspace.py');
    has('workspace-cleanup/assets/candidates.md');
    const text = skill('workspace-cleanup');
    assert.ok(text.includes('scan_workspace.py --delete'));
    assert.match(text, /never deletes on its own/);
  });

  it('pull-request keeps its description template in assets', () => {
    has('pull-request/assets/pr-body.md');
    const body = readFile(dir, '.agents/skills/pull-request/assets/pr-body.md');
    for (const heading of ['## Summary', '## Changes Made', '## Testing Plan', '## Revert Plan', '## Other Remarks']) {
      assert.ok(body.includes(heading), heading);
    }
    assert.ok(skill('pull-request').includes('assets/pr-body.md'));
    assert.ok(!skill('pull-request').includes('## Thinking Process'));
  });

  it('knowledge ships entry templates and points at them', () => {
    for (const name of ['decision', 'context-note', 'best-practice', 'business-flow']) {
      has(`knowledge/assets/${name}.md`);
      assert.ok(skill('knowledge').includes(`assets/${name}.md`), name);
    }
  });

  it('cleanup is wired into spec-implement and the delivery task', () => {
    assert.ok(skill('spec-implement').includes('/code-cleanup --diff'));
    assert.ok(skill('spec-planner').includes('/code-cleanup --diff'));
    assert.ok(readFile(dir, '.agents/skills/spec-planner/assets/delivery-task.md').includes('/code-cleanup --diff {{target}}'));
  });

  const NEW_SKILLS = {
    'spec-explain': { command: '/spec-explain', files: ['assets/explanation.md'] },
    explain: { command: '/explain', files: ['scripts/locate.py', 'assets/technical.md', 'assets/non-technical.md'] },
    feasibility: { command: '/feasibility', files: ['assets/report.md'] },
    'code-cleanup': { command: '/code-cleanup', files: ['scripts/comments.py', 'assets/candidates.md'] },
  };

  for (const [name, spec] of Object.entries(NEW_SKILLS)) {
    it(`installs ${name} with its interface, scripts, and templates`, () => {
      has(`${name}/SKILL.md`);
      has(`${name}/agents/openai.yaml`);
      for (const file of spec.files) has(`${name}/${file}`);
      const text = skill(name);
      assert.match(text, new RegExp(`^---\\nname: ${name}\\n`));
      assert.ok(text.split('---')[1].includes(spec.command), 'description names the command');
      assert.ok(readFile(dir, `.agents/skills/${name}/agents/openai.yaml`).includes(`default_prompt: "${spec.command}"`));
    });
  }

  it('the new skills call each other by path and keep their read-only mandates', () => {
    assert.ok(skill('spec-explain').includes('spec-planner/scripts/specs.py'));
    assert.ok(skill('feasibility').includes('explain/scripts/locate.py'));
    assert.ok(skill('code-cleanup').includes('explain/scripts/locate.py stack'));
    for (const name of ['spec-explain', 'explain', 'feasibility']) assert.match(skill(name), /Read-Only/);
    assert.match(skill('code-cleanup'), /Approval First/);
  });

  it('spec-planner ships specs.py with its templates and the spec skills call it', () => {
    has('spec-planner/scripts/specs.py');
    for (const name of ['spec-implement', 'spec-compile', 'spec-explain']) {
      assert.ok(skill(name).includes('spec-planner/scripts/specs.py'), name);
    }
  });

  it('installed scripts run from the workspace with python3', { skip: !hasPython }, () => {
    const run = (rel, ...args) => JSON.parse(execFileSync('python3', [`${dir}/.agents/skills/${rel}`, ...args], { cwd: dir, encoding: 'utf8' }));
    assert.equal(run('spec-planner/scripts/specs.py', 'next-phase').next, '001');
    const phase = run('spec-planner/scripts/specs.py', 'new-phase', '--repo', 'demo', '--title', 'Demo', '--branch', 'feat/demo', '--target', 'trunk');
    assert.equal(phase.phase, 'p-001-demo');
    const task = run('spec-planner/scripts/specs.py', 'new-task', 'p-001', '--title', 'Do it', '--type', 'Enhancement', '--paths', 'a.js');
    assert.deepEqual([task.task, task.delivery], ['t-001', 't-002']);
    assert.deepEqual(run('spec-planner/scripts/specs.py', 'waves', 'p-001').waves, [['t-001'], ['t-002']]);
    assert.equal(run('workspace-cleanup/scripts/scan_workspace.py').root.length > 0, true);
  });
});
