import { describe, it, before, after } from 'node:test';
import assert from 'node:assert/strict';
import { makeTmp, cleanup, runCli, fileExists, readFile, writeFile } from './helpers.js';

describe('coordination skills', () => {
  let dir;
  before(async () => {
    dir = makeTmp();
    await runCli(['init', '--yes', '--providers', 'claude', '--no-link'], dir);
  });
  after(() => cleanup(dir));

  it('installs the agent-coordination skill with its script', () => {
    assert.ok(fileExists(dir, '.agents/skills/agent-coordination/SKILL.md'));
    assert.ok(fileExists(dir, '.agents/skills/agent-coordination/scripts/coord.py'));
    assert.ok(fileExists(dir, '.agents/skills/agent-coordination/references/protocol.md'));
  });

  it('AGENTS.md managed block carries the coordination principle', () => {
    const agents = readFile(dir, 'AGENTS.md');
    assert.ok(agents.includes('agent-coordination'), 'mentions the skill');
    assert.ok(agents.includes('coord.py whoami'), 'session-start whoami rule');
  });

  it('spec-planner checks active work and templates record paths and dependencies', () => {
    const skill = readFile(dir, '.agents/skills/spec-planner/SKILL.md');
    assert.ok(skill.includes('Check Active & Planned Work'));
    assert.ok(skill.includes('## Affected Paths'));
    assert.ok(skill.includes('## Depends On'));
    const task = readFile(dir, '.agents/skills/spec-planner/assets/task.md');
    assert.ok(task.includes('## Affected Paths') && task.includes('## Depends On'));
  });

  it('spec-planner ships its templates and script and the skill calls them', () => {
    for (const rel of ['scripts/specs.py', 'assets/impl.md', 'assets/task.md', 'assets/delivery-task.md']) {
      assert.ok(fileExists(dir, `.agents/skills/spec-planner/${rel}`), rel);
    }
    const skill = readFile(dir, '.agents/skills/spec-planner/SKILL.md');
    for (const needle of ['specs.py new-phase', 'specs.py new-task', 'specs.py lint', 'assets/']) {
      assert.ok(skill.includes(needle), `mentions ${needle}`);
    }
  });

  it('spec-implement claims, heartbeats, checks answers, and releases', () => {
    const skill = readFile(dir, '.agents/skills/spec-implement/SKILL.md');
    for (const needle of ['coord.py claim', 'heartbeat', 'answers --mine', 'release', '--via cli']) {
      assert.ok(skill.includes(needle), `mentions ${needle}`);
    }
  });

  it('spec-implement and spec-compile run on specs.py and ship their templates', () => {
    const implement = readFile(dir, '.agents/skills/spec-implement/SKILL.md');
    for (const needle of ['specs.py set-status', 'specs.py normalize', 'specs.py resolve', 'assets/task-metadata.md']) {
      assert.ok(implement.includes(needle), `spec-implement mentions ${needle}`);
    }
    const compile = readFile(dir, '.agents/skills/spec-compile/SKILL.md');
    assert.ok(compile.includes('specs.py compile') && compile.includes('Manual Fallback'));
    for (const rel of ['spec-implement/assets/task-metadata.md', 'spec-compile/assets/completed.md', 'spec-compile/assets/incomplete.md']) {
      assert.ok(fileExists(dir, `.agents/skills/${rel}`), rel);
    }
  });

  it('spec-implement plans parallel waves with worktree subagents and a sequential fallback', () => {
    const implement = readFile(dir, '.agents/skills/spec-implement/SKILL.md');
    for (const needle of ['## Step 6a', 'specs.py waves', '<working-branch>--t-###', 'Sequential mode', 'worktree isolation',
      'Never run tasks with overlapping']) {
      assert.ok(implement.includes(needle), `spec-implement mentions ${needle}`);
    }
    for (const needle of ['coord.py claim', 'coord.py release']) assert.ok(implement.includes(needle), needle);
    const planner = readFile(dir, '.agents/skills/spec-planner/SKILL.md');
    assert.ok(planner.includes('specs.py waves') && planner.includes('Parallel Waves'));
    assert.ok(readFile(dir, '.agents/skills/spec-planner/assets/impl.md').includes('## Parallel Waves'));
  });

  it('both skills degrade gracefully without coordination', () => {
    for (const name of ['spec-planner', 'spec-implement']) {
      assert.ok(readFile(dir, `.agents/skills/${name}/SKILL.md`).includes('in-progress.md'), name);
    }
  });

  it('in-progress template has session and claim fields', () => {
    const t = readFile(dir, 'specs/in-progress.md');
    assert.ok(t.includes('Session') && t.includes('Claim'));
  });

  it('update backs up a locally edited spec-planner and refreshes it', async () => {
    const rel = '.agents/skills/spec-planner/SKILL.md';
    writeFile(dir, rel, readFile(dir, rel) + '\n<!-- local edit -->');
    const { code } = await runCli(['update', '--yes', '--no-link'], dir);
    assert.equal(code, 0);
    assert.ok(fileExists(dir, rel + '.bak'));
    assert.ok(!readFile(dir, rel).includes('<!-- local edit -->'));
  });

  it('update leaves user content above the managed block alone', async () => {
    writeFile(dir, 'AGENTS.md', '# Mine\n\nkeep me\n\n' + readFile(dir, 'AGENTS.md'));
    await runCli(['update', '--yes', '--no-link'], dir);
    assert.ok(readFile(dir, 'AGENTS.md').includes('keep me'));
  });
});
