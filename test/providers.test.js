import { describe, it } from 'node:test';
import assert from 'node:assert/strict';
import { PROVIDERS, VALID_PROVIDERS, getProvider } from '../src/core/providers.js';
import { agents, resolve, validate } from '../src/core/models.js';

const MODEL_ID = /^[a-z0-9-]+\/[A-Za-z0-9][A-Za-z0-9._:-]*$/;

describe('opencode provider registry', () => {
  it('is listed after the existing providers', () => {
    assert.deepEqual(VALID_PROVIDERS, ['claude', 'antigravity', 'codex', 'opencode']);
  });

  it('reads AGENTS.md and .agents/skills natively and writes .opencode/agents', () => {
    const p = getProvider('opencode');
    assert.equal(p.doc, 'AGENTS.md');
    assert.equal(p.agentsDir, '.opencode/agents');
    assert.equal(p.skillsDir, '.agents/skills');
    assert.equal(p.native, false);
    assert.equal(p.nativeSkills, true);
    assert.equal(typeof p.transformAgent, 'function');
  });
});

describe('transformOpencode', () => {
  const fm = { name: 'tech-lead-orchestrator', description: 'Lead', tools: ['*'], model: 'stale' };

  it('keeps name and description, sets subagent mode, drops tools, resolves the model', () => {
    const out = PROVIDERS.opencode.transformAgent(fm, 'tech-lead-orchestrator');
    assert.equal(out.name, 'tech-lead-orchestrator');
    assert.equal(out.description, 'Lead');
    assert.equal(out.mode, 'subagent');
    assert.equal('tools' in out, false);
    assert.equal(out.model, resolve('tech-lead-orchestrator', 'opencode'));
  });

  it('does not mutate its input', () => {
    const copy = structuredClone(fm);
    PROVIDERS.opencode.transformAgent(fm, 'tech-lead-orchestrator');
    assert.deepEqual(fm, copy);
  });

  it('keeps the incoming model when the agent is unknown', () => {
    const out = PROVIDERS.opencode.transformAgent({ name: 'x', description: 'y', model: 'keep' }, 'not-an-agent');
    assert.equal(out.model, 'keep');
  });
});

describe('models.json opencode column', () => {
  it('has a provider/model id for every agent', () => {
    for (const agent of agents()) {
      const id = resolve(agent, 'opencode');
      assert.match(id, MODEL_ID, `${agent} -> ${id}`);
    }
  });

  it('mirrors the claude column under the anthropic/ prefix', () => {
    for (const agent of agents()) {
      assert.equal(resolve(agent, 'opencode'), `anthropic/${resolve(agent, 'claude')}`);
    }
  });

  it('passes models.validate for every provider', () => {
    assert.deepEqual(validate(VALID_PROVIDERS), []);
  });
});

describe('existing provider transforms are unchanged', () => {
  const fm = { name: 'qa-engineer', description: 'QA', tools: ['x'] };

  it('claude drops tools and sets the claude model', () => {
    const out = PROVIDERS.claude.transformAgent(fm, 'qa-engineer');
    assert.equal('tools' in out, false);
    assert.equal(out.model, 'claude-sonnet-4-6');
    assert.equal('mode' in out, false);
  });

  it('antigravity sets tools to ["*"]', () => {
    const out = PROVIDERS.antigravity.transformAgent(fm, 'qa-engineer');
    assert.deepEqual(out.tools, ['*']);
    assert.equal(out.model, 'gemini-2.0-flash');
  });

  it('codex keeps frontmatter and sets the codex model', () => {
    const out = PROVIDERS.codex.transformAgent(fm, 'qa-engineer');
    assert.deepEqual(out.tools, ['x']);
    assert.equal(out.model, 'gpt-5.3-codex');
  });
});
