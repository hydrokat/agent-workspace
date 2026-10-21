import { describe, it, before, after } from 'node:test';
import assert from 'node:assert/strict';
import { unlinkSync } from 'node:fs';
import { join } from 'node:path';
import { makeTmp, cleanup, runCli, fileExists, readFile, writeFile } from './helpers.js';

describe('update', () => {
  let dir;
  before(async () => {
    dir = makeTmp();
    await runCli(['init', '--yes', '--providers', 'claude', '--no-link'], dir);
  });
  after(() => cleanup(dir));

  it('backs up a locally-modified managed file on update', async () => {
    const rel = '.claude/agents/tech-lead-orchestrator.md';
    const original = readFile(dir, rel);
    writeFile(dir, rel, original + '\n<!-- local edit -->');

    const { code } = await runCli(['update', '--yes', '--no-link'], dir);
    assert.equal(code, 0, 'update exits 0');

    assert.ok(fileExists(dir, rel + '.bak'), '.bak created for modified file');
    const restored = readFile(dir, rel);
    assert.ok(!restored.includes('<!-- local edit -->'), 'managed file restored to shipped version');
  });

  it('does not modify specs/ content', async () => {
    const specContent = '# My spec\n\nDo not touch.';
    writeFile(dir, 'specs/phase-001/impl.md', specContent);

    await runCli(['update', '--yes', '--no-link'], dir);

    const after = readFile(dir, 'specs/phase-001/impl.md');
    assert.equal(after, specContent, 'specs content untouched');
  });

  it('does not modify user knowledgebase entries', async () => {
    const kbContent = '# My custom note\n\nProject specific content.';
    writeFile(dir, 'knowledgebase/business-flows/my-flow.md', kbContent);

    await runCli(['update', '--yes', '--no-link'], dir);

    const after = readFile(dir, 'knowledgebase/business-flows/my-flow.md');
    assert.equal(after, kbContent, 'user kb entry untouched');
  });

  it('keeps edited knowledgebase notes and refreshes only the guideline documents', async () => {
    const mine = {
      'knowledgebase/context-history/key-decisions.md': '# Key Decisions\n\n### 2026-01-01: Mine\n',
      'knowledgebase/context-history/task-completion-log.md': '# Log\n\nmine\n',
      'knowledgebase/best-practices/lessons-learned.md': '# Lessons\n\nmine\n',
      'knowledgebase/guidelines/README.md': '# Guidelines\n\nmine\n',
    };
    for (const [rel, text] of Object.entries(mine)) writeFile(dir, rel, text);
    const guideline = 'knowledgebase/guidelines/security.md';
    writeFile(dir, guideline, readFile(dir, guideline) + '\n<!-- local edit -->');

    const { code } = await runCli(['update', '--yes', '--no-link'], dir);
    assert.equal(code, 0);

    for (const [rel, text] of Object.entries(mine)) {
      assert.equal(readFile(dir, rel), text, `${rel} untouched`);
      assert.ok(!fileExists(dir, rel + '.bak'), `${rel} not backed up`);
    }
    assert.ok(fileExists(dir, guideline + '.bak'), 'edited guideline backed up');
    assert.ok(!readFile(dir, guideline).includes('<!-- local edit -->'), 'guideline refreshed');
  });

  it('recreates a deleted knowledgebase note from the template', async () => {
    const rel = 'knowledgebase/context-history/key-decisions.md';
    unlinkSync(join(dir, rel));
    await runCli(['update', '--yes', '--no-link'], dir);
    assert.ok(readFile(dir, rel).includes('# Key Decisions'));
  });

  it('installs release versioning guideline on update', async () => {
    unlinkSync(join(dir, 'knowledgebase/guidelines/release-versioning.md'));

    const { code } = await runCli(['update', '--yes', '--no-link'], dir);
    assert.equal(code, 0, 'update exits 0');
    assert.ok(
      fileExists(dir, 'knowledgebase/guidelines/release-versioning.md'),
      'release versioning guideline installed'
    );
  });

  it('preserves AGENTS.md content outside managed markers', async () => {
    const agentsMd = readFile(dir, 'AGENTS.md');
    const withCustom = agentsMd.replace(
      '## Workspace Overview',
      '## Workspace Overview\n\n<!-- custom section -->'
    );
    writeFile(dir, 'AGENTS.md', withCustom);

    await runCli(['update', '--yes', '--no-link'], dir);

    const after = readFile(dir, 'AGENTS.md');
    assert.ok(after.includes('<!-- custom section -->'), 'custom content outside managed block preserved');
  });
});

describe('update adds new agents', () => {
  let dir;
  before(async () => {
    dir = makeTmp();
    await runCli(['init', '--yes', '--providers', 'claude', '--no-link'], dir);
  });
  after(() => cleanup(dir));

  it('generated agents exist after update', async () => {
    const { code } = await runCli(['update', '--yes', '--no-link'], dir);
    assert.equal(code, 0);
    // All template agents should be present
    for (const agent of ['tech-lead-orchestrator', 'senior-backend-dev', 'terminal-agent']) {
      assert.ok(
        fileExists(dir, `.claude/agents/${agent}.md`),
        `${agent} present after update`
      );
    }
  });
});

describe('update cleans up stale skills and agents', () => {
  let dir;
  before(async () => {
    dir = makeTmp();
    await runCli(['init', '--yes', '--providers', 'claude,antigravity,codex', '--no-link'], dir);
  });
  after(() => cleanup(dir));

  it('removes stale skill directory from .agents/skills and .claude/skills', async () => {
    writeFile(dir, '.agents/skills/stale-skill/SKILL.md', '# Stale');
    writeFile(dir, '.claude/skills/stale-skill/SKILL.md', '# Stale');

    const manifestRel = '.agent-workspace/manifest.json';
    const manifest = JSON.parse(readFile(dir, manifestRel));
    manifest.managedFiles.push('.agents/skills/stale-skill', '.claude/skills/stale-skill');
    writeFile(dir, manifestRel, JSON.stringify(manifest, null, 2) + '\n');

    const { code, stdout } = await runCli(['update', '--yes', '--no-link'], dir);
    assert.equal(code, 0);
    assert.ok(!fileExists(dir, '.agents/skills/stale-skill'), 'stale skill removed from .agents/skills');
    assert.ok(!fileExists(dir, '.claude/skills/stale-skill'), 'stale skill removed from .claude/skills');
    assert.ok(stdout.includes('Removed stale skill: .agents/skills/stale-skill'));
    assert.ok(stdout.includes('Removed stale skill: .claude/skills/stale-skill'));

    const updatedManifest = JSON.parse(readFile(dir, manifestRel));
    assert.ok(!updatedManifest.managedFiles.includes('.agents/skills/stale-skill'));
    assert.ok(!updatedManifest.managedFiles.includes('.claude/skills/stale-skill'));
  });

  it('removes stale agent directory from .agents/agents and prunes manifest', async () => {
    writeFile(dir, '.agents/agents/stale-agent/AGENT.md', '# Stale');
    const manifestRel = '.agent-workspace/manifest.json';
    const manifest = JSON.parse(readFile(dir, manifestRel));
    manifest.managedFiles.push('.agents/agents/stale-agent/AGENT.md');
    writeFile(dir, manifestRel, JSON.stringify(manifest, null, 2) + '\n');

    const { code, stdout } = await runCli(['update', '--yes', '--no-link'], dir);
    assert.equal(code, 0);
    assert.ok(!fileExists(dir, '.agents/agents/stale-agent'), 'stale agent removed from .agents/agents');
    assert.ok(stdout.includes('Removed stale agent: .agents/agents/stale-agent'));

    const updatedManifest = JSON.parse(readFile(dir, manifestRel));
    assert.ok(!updatedManifest.managedFiles.includes('.agents/agents/stale-agent/AGENT.md'));
  });

  it('removes stale file inside an existing skill directory while preserving .bak files', async () => {
    const staleFile = '.agents/skills/spec-planner/scripts/old-helper.py';
    const backupFile = '.agents/skills/spec-planner/scripts/old-helper.py.bak';
    writeFile(dir, staleFile, '# old helper');
    writeFile(dir, backupFile, '# backup');

    const { code, stdout } = await runCli(['update', '--yes', '--no-link'], dir);
    assert.equal(code, 0);
    assert.ok(!fileExists(dir, staleFile), 'stale file inside skill removed');
    assert.ok(fileExists(dir, backupFile), 'backup file preserved');
    assert.ok(stdout.includes('Removed stale skill file: ' + staleFile));
  });

  it('removes legacy .gemini/skills directory and prunes manifest entries', async () => {
    writeFile(dir, '.gemini/skills/specs-planner/SKILL.md', '# Legacy');
    const manifestRel = '.agent-workspace/manifest.json';
    const manifest = JSON.parse(readFile(dir, manifestRel));
    manifest.managedFiles.push('.gemini/skills/specs-planner');
    writeFile(dir, manifestRel, JSON.stringify(manifest, null, 2) + '\n');

    const { code, stdout } = await runCli(['update', '--yes', '--no-link'], dir);
    assert.equal(code, 0);
    assert.ok(!fileExists(dir, '.gemini/skills/specs-planner'), 'legacy skill inside .gemini/skills removed');
    assert.ok(!fileExists(dir, '.gemini/skills'), 'empty legacy .gemini/skills directory removed');
    assert.ok(stdout.includes('Removed stale skill: .gemini/skills/specs-planner'));

    const updatedManifest = JSON.parse(readFile(dir, manifestRel));
    assert.ok(!updatedManifest.managedFiles.includes('.gemini/skills/specs-planner'));
  });

  it('--dry-run does not delete stale skills or files', async () => {
    writeFile(dir, '.agents/skills/dry-stale/SKILL.md', '# Stale');
    const { code } = await runCli(['update', '--yes', '--no-link', '--dry-run'], dir);
    assert.equal(code, 0);
    assert.ok(fileExists(dir, '.agents/skills/dry-stale'), 'stale skill remains after dry-run');
    // clean up
    cleanup(join(dir, '.agents/skills/dry-stale'));
  });
});
