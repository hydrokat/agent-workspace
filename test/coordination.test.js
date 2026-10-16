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
  });

  it('spec-implement claims, heartbeats, checks answers, and releases', () => {
    const skill = readFile(dir, '.agents/skills/spec-implement/SKILL.md');
    for (const needle of ['coord.py claim', 'heartbeat', 'answers --mine', 'release', '--via cli']) {
      assert.ok(skill.includes(needle), `mentions ${needle}`);
    }
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
