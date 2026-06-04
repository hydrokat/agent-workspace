import { describe, it, before, after } from 'node:test';
import assert from 'node:assert/strict';
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
