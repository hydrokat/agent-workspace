import { describe, it, before, after } from 'node:test';
import assert from 'node:assert/strict';
import { makeTmp, cleanup, runCli, fileExists, readFile, writeFile } from './helpers.js';

describe('init', () => {
  let dir;
  before(() => { dir = makeTmp(); });
  after(() => cleanup(dir));

  it('creates workspace structure', async () => {
    const { code } = await runCli(['init', '--yes', '--providers', 'claude', '--no-link'], dir);
    assert.equal(code, 0, 'should exit 0');

    // Dirs
    assert.ok(fileExists(dir, '.agents/agents'), '.agents/agents exists');
    assert.ok(fileExists(dir, '.agents/skills'), '.agents/skills exists');
    assert.ok(fileExists(dir, 'knowledgebase'), 'knowledgebase exists');
    assert.ok(fileExists(dir, 'specs'), 'specs exists');
    assert.ok(fileExists(dir, '.tmp'), '.tmp exists');

    // Starter files
    assert.ok(fileExists(dir, 'AGENTS.md'), 'AGENTS.md created');
    assert.ok(fileExists(dir, 'WORKFLOW.md'), 'WORKFLOW.md created');
    assert.ok(fileExists(dir, 'specs/in-progress.md'), 'specs/in-progress.md created');
    assert.ok(
      fileExists(dir, 'knowledgebase/guidelines/release-versioning.md'),
      'release versioning guideline created'
    );

    // Canonical agents installed
    assert.ok(fileExists(dir, '.agents/agents/tech-lead-orchestrator/AGENT.md'), 'canonical agent installed');
    assert.ok(fileExists(dir, '.agents/skills/code-review-remediation/SKILL.md'), 'code review remediation skill installed');

    // Claude provider output
    assert.ok(fileExists(dir, '.claude/agents/tech-lead-orchestrator.md'), 'claude agent generated');

    // Manifest
    assert.ok(fileExists(dir, '.agent-workspace/manifest.json'), 'manifest written');
    const manifest = JSON.parse(readFile(dir, '.agent-workspace/manifest.json'));
    assert.deepEqual(manifest.providers, ['claude']);
  });

  it('model injected in generated claude agent', async () => {
    const content = readFile(dir, '.claude/agents/tech-lead-orchestrator.md');
    assert.ok(content.includes('model:'), 'model field present');
    assert.ok(content.includes('claude-opus-4-7') || content.includes('opus'), 'correct model injected');
  });

  it('claude agent has no tools field', async () => {
    const content = readFile(dir, '.claude/agents/senior-backend-dev.md');
    assert.ok(!content.match(/^tools:/m), 'tools field removed for claude');
  });

  it('guards against re-init without --force', async () => {
    const { code, stdout, stderr } = await runCli(['init', '--yes', '--no-link'], dir);
    const output = stdout + stderr;
    assert.ok(output.includes('manifest') || output.includes('update'), 'warns about existing manifest');
  });
});

describe('init with multiple providers', () => {
  let dir;
  before(async () => {
    dir = makeTmp();
    await runCli(['init', '--yes', '--providers', 'claude,antigravity,codex', '--no-link'], dir);
  });
  after(() => cleanup(dir));

  it('generates claude outputs', () => {
    assert.ok(fileExists(dir, '.claude/agents/tech-lead-orchestrator.md'));
  });

  it('generates antigravity outputs with tools field', () => {
    assert.ok(fileExists(dir, '.gemini/agents/tech-lead-orchestrator.md'));
    const content = readFile(dir, '.gemini/agents/tech-lead-orchestrator.md');
    assert.ok(content.includes('tools:'), 'antigravity agent has tools field');
    assert.ok(content.includes('"*"'), 'antigravity tools is wildcard');
  });

  it('antigravity does not create a separate skills directory', () => {
    assert.ok(!fileExists(dir, '.gemini/skills'), 'no .gemini/skills created for antigravity');
  });

  it('injects model into codex native agent', () => {
    const content = readFile(dir, '.agents/agents/terminal-agent/AGENT.md');
    assert.ok(content.includes('model:'), 'codex agent has model field');
  });

  it('manifest records all three providers', () => {
    const manifest = JSON.parse(readFile(dir, '.agent-workspace/manifest.json'));
    assert.ok(manifest.providers.includes('claude'));
    assert.ok(manifest.providers.includes('antigravity'));
    assert.ok(manifest.providers.includes('codex'));
  });
});
