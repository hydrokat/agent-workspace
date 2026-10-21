import { describe, it, before, after } from 'node:test';
import assert from 'node:assert/strict';
import { mkdirSync, unlinkSync, writeFileSync } from 'node:fs';
import { join } from 'node:path';
import { makeTmp, cleanup, runCli, fileExists, readFile, writeFile } from './helpers.js';

describe('doctor', () => {
  let dir;
  before(async () => {
    dir = makeTmp();
    await runCli(['init', '--yes', '--providers', 'claude', '--no-link'], dir);
  });
  after(() => cleanup(dir));

  it('passes on a correctly initialized workspace', async () => {
    const { code } = await runCli(['doctor'], dir);
    assert.equal(code, 0, 'doctor exits 0 on clean workspace');
  });

  it('fails when a generated agent is deleted', async () => {
    const agentPath = join(dir, '.claude/agents/tech-lead-orchestrator.md');
    unlinkSync(agentPath);

    const { code, stdout, stderr } = await runCli(['doctor'], dir);
    assert.notEqual(code, 0, 'doctor exits non-zero');
    const output = stdout + stderr;
    assert.ok(output.includes('tech-lead-orchestrator'), 'reports missing agent');

    // restore
    await runCli(['update', '--yes', '--no-link'], dir);
  });

  it('fails when model value is wrong', async () => {
    const rel = '.claude/agents/terminal-agent.md';
    const content = readFile(dir, rel);
    const corrupted = content.replace(/^model:.*$/m, 'model: wrong-model-id');
    writeFile(dir, rel, corrupted);

    const { code, stdout, stderr } = await runCli(['doctor'], dir);
    assert.notEqual(code, 0, 'doctor exits non-zero on bad model');
    const output = stdout + stderr;
    assert.ok(output.includes('terminal-agent'), 'reports which agent has bad model');

    // restore
    await runCli(['update', '--yes', '--no-link'], dir);
  });

  it('--json emits structured output', async () => {
    const { stdout } = await runCli(['doctor', '--json'], dir);
    const lines = stdout.split('\n').filter(Boolean);
    assert.ok(lines.length > 0, 'produces json lines');
    const first = JSON.parse(lines[0]);
    assert.ok(typeof first.level === 'string', 'json records have level');
    assert.ok(typeof first.message === 'string', 'json records have message');
  });

  it('fails when a template skill SKILL.md is missing', async () => {
    const skillMd = join(dir, '.agents/skills/spec-implement/SKILL.md');
    unlinkSync(skillMd);

    const { code, stdout, stderr } = await runCli(['doctor'], dir);
    assert.notEqual(code, 0, 'doctor exits non-zero on missing skill');
    const output = stdout + stderr;
    assert.ok(output.includes('spec-implement'), 'reports missing skill');

    // restore
    await runCli(['update', '--yes', '--no-link'], dir);
  });

  it('fails when a stale skill directory exists in .agents/skills', async () => {
    const staleDir = join(dir, '.agents/skills/stale-skill');
    mkdirSync(staleDir, { recursive: true });
    writeFileSync(join(staleDir, 'SKILL.md'), '# Stale');

    const { code, stdout, stderr } = await runCli(['doctor'], dir);
    assert.notEqual(code, 0, 'doctor exits non-zero on stale skill directory');
    const output = stdout + stderr;
    assert.ok(output.includes('stale-skill'), 'reports stale skill');

    // clean up
    cleanup(staleDir);
  });

  it('fails when a legacy .gemini/skills directory exists', async () => {
    const legacyDir = join(dir, '.gemini/skills');
    mkdirSync(legacyDir, { recursive: true });

    const { code, stdout, stderr } = await runCli(['doctor'], dir);
    assert.notEqual(code, 0, 'doctor exits non-zero on legacy .gemini/skills');
    const output = stdout + stderr;
    assert.ok(output.includes('.gemini/skills'), 'reports legacy .gemini/skills');

    // clean up
    cleanup(legacyDir);
  });
});
