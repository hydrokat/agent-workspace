import { resolve } from './models.js';

function transformClaude(fm, agentName) {
  const out = { ...fm };
  delete out.tools;
  const model = resolve(agentName, 'claude');
  if (model) out.model = model;
  return out;
}

function transformAntigravity(fm, agentName) {
  const out = { ...fm };
  out.tools = ['*'];
  const model = resolve(agentName, 'antigravity');
  if (model) out.model = model;
  return out;
}

function transformCodex(fm, agentName) {
  const out = { ...fm };
  const model = resolve(agentName, 'codex');
  if (model) out.model = model;
  return out;
}

function transformOpencode(fm, agentName) {
  const out = { ...fm };
  delete out.tools;
  out.mode = 'subagent';
  const model = resolve(agentName, 'opencode');
  if (model) out.model = model;
  return out;
}

export const PROVIDERS = {
  claude: {
    doc: 'CLAUDE.md',
    agentsDir: '.claude/agents',
    skillsDir: '.claude/skills',
    native: false,
    nativeSkills: false,
    transformAgent: transformClaude,
  },
  antigravity: {
    doc: 'GEMINI.md',
    agentsDir: '.gemini/agents',
    skillsDir: '.agents/skills',
    native: false,
    nativeSkills: true,
    transformAgent: transformAntigravity,
  },
  codex: {
    doc: 'AGENTS.md',
    agentsDir: '.agents/agents',
    skillsDir: '.agents/skills',
    native: true,
    nativeSkills: true,
    transformAgent: transformCodex,
  },
  opencode: {
    doc: 'AGENTS.md',
    agentsDir: '.opencode/agents',
    skillsDir: '.agents/skills',
    native: false,
    nativeSkills: true,
    userOwnedDir: true,
    expectedFrontmatter: { mode: 'subagent' },
    transformAgent: transformOpencode,
  },
};

export const VALID_PROVIDERS = Object.keys(PROVIDERS);

export function getProvider(name) {
  return PROVIDERS[name] ?? null;
}
