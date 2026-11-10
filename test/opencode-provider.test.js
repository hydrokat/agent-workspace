import { describe, it, before, after } from 'node:test';
import assert from 'node:assert/strict';
import { existsSync, mkdirSync, readdirSync, writeFileSync } from 'node:fs';
import { join } from 'node:path';
import { makeTmp, cleanup, runCli as runCliRaw, fileExists, readFile, writeFile } from './helpers.js';
import { parse as parseFm } from '../src/core/frontmatter.js';
import { agents, resolve } from '../src/core/models.js';
import { readFileSync } from 'node:fs';
import { dirname } from 'node:path';
import { fileURLToPath } from 'node:url';

const ROOT = dirname(dirname(fileURLToPath(import.meta.url)));

const ISOLATED_HOME = makeTmp();
process.on('exit', () => cleanup(ISOLATED_HOME));
const runCli = (args, cwd) => runCliRaw(args, cwd, {
  ...process.env,
  HOME: ISOLATED_HOME,
  XDG_STATE_HOME: join(ISOLATED_HOME, 'state'),
});

function manifestOf(dir) {
  return JSON.parse(readFile(dir, '.agent-workspace/manifest.json'));
}

describe('opencode: init', () => {
  let dir;
  before(async () => {
    dir = makeTmp();
    const r = await runCli(['init', '--yes', '--providers', 'claude,opencode', '--no-link'], dir);
    assert.equal(r.code, 0, r.stdout + r.stderr);
  });
  after(() => cleanup(dir));

  it('writes one agent file per agent under .opencode/agents', () => {
    const files = readdirSync(join(dir, '.opencode/agents')).filter((f) => f.endsWith('.md'));
    assert.equal(files.length, agents().length);
    for (const agent of agents()) assert.ok(fileExists(dir, `.opencode/agents/${agent}.md`), agent);
  });

  it('generated agents have name, description, subagent mode, and the resolved model', () => {
    for (const agent of agents()) {
      const { frontmatter } = parseFm(readFile(dir, `.opencode/agents/${agent}.md`));
      assert.equal(frontmatter.name, agent);
      assert.ok(frontmatter.description);
      assert.equal(frontmatter.mode, 'subagent');
      assert.equal(frontmatter.model, resolve(agent, 'opencode'));
      assert.equal('tools' in frontmatter, false);
    }
  });

  it('creates no OPENCODE.md, no .opencode/skills, and keeps canonical skills in .agents/skills', () => {
    assert.equal(fileExists(dir, 'OPENCODE.md'), false);
    assert.equal(fileExists(dir, '.opencode/skills'), false);
    assert.ok(fileExists(dir, '.agents/skills/code-review/SKILL.md'));
  });

  it('records opencode in the manifest', () => {
    assert.deepEqual(manifestOf(dir).providers, ['claude', 'opencode']);
  });

  it('doctor passes', async () => {
    const { code, stdout, stderr } = await runCli(['doctor'], dir);
    assert.equal(code, 0, stdout + stderr);
    assert.ok((stdout + stderr).includes('[opencode] agent exists: tech-lead-orchestrator'));
  });
});

describe('opencode: update never touches user-owned content', () => {
  let dir;
  before(async () => {
    dir = makeTmp();
    await runCli(['init', '--yes', '--providers', 'opencode', '--no-link'], dir);
    writeFile(dir, '.opencode/agents/my-own-agent.md', '---\ndescription: mine\nmode: primary\n---\nhello\n');
    writeFile(dir, '.opencode/skills/my-skill/SKILL.md', '---\nname: my-skill\ndescription: mine\n---\nbody\n');
    writeFile(dir, '.opencode/opencode.json', '{"theme":"x"}\n');
    mkdirSync(join(dir, '.opencode/agents/team'), { recursive: true });
    writeFile(dir, '.opencode/agents/team/reviewer.md', '---\ndescription: nested\n---\nx\n');
  });
  after(() => cleanup(dir));

  it('keeps user agents, nested agent dirs, skills, and config after update', async () => {
    const { code, stdout, stderr } = await runCli(['update', '--yes', '--no-link'], dir);
    assert.equal(code, 0, stdout + stderr);
    assert.ok(fileExists(dir, '.opencode/agents/my-own-agent.md'));
    assert.ok(fileExists(dir, '.opencode/agents/team/reviewer.md'));
    assert.ok(fileExists(dir, '.opencode/skills/my-skill/SKILL.md'));
    assert.equal(readFile(dir, '.opencode/opencode.json'), '{"theme":"x"}\n');
  });

  it('prunes a stale agent that an earlier release managed', async () => {
    writeFile(dir, '.opencode/agents/retired-agent.md', '---\ndescription: old\n---\nx\n');
    const manifest = manifestOf(dir);
    manifest.managedFiles.push('.opencode/agents/retired-agent.md');
    writeFileSync(join(dir, '.agent-workspace/manifest.json'), JSON.stringify(manifest, null, 2));

    const { code, stdout, stderr } = await runCli(['update', '--yes', '--no-link'], dir);
    assert.equal(code, 0, stdout + stderr);
    assert.equal(existsSync(join(dir, '.opencode/agents/retired-agent.md')), false);
    assert.equal(manifestOf(dir).managedFiles.includes('.opencode/agents/retired-agent.md'), false);
    assert.ok(fileExists(dir, '.opencode/agents/my-own-agent.md'));
  });

  it('never deletes a directory even when the manifest lists it', async () => {
    const manifest = manifestOf(dir);
    manifest.managedFiles.push('.opencode/agents/team');
    writeFileSync(join(dir, '.agent-workspace/manifest.json'), JSON.stringify(manifest, null, 2));
    const { code, stdout, stderr } = await runCli(['update', '--yes', '--no-link'], dir);
    assert.equal(code, 0, stdout + stderr);
    assert.ok(fileExists(dir, '.opencode/agents/team/reviewer.md'));
  });

  it('backs up a locally edited managed agent before overwriting it', async () => {
    const rel = '.opencode/agents/terminal-agent.md';
    writeFile(dir, rel, readFile(dir, rel) + '\nlocal edit\n');
    const { code } = await runCli(['update', '--yes', '--no-link'], dir);
    assert.equal(code, 0);
    assert.ok(fileExists(dir, rel + '.bak'));
    assert.equal(readFile(dir, rel).includes('local edit'), false);
  });
});

describe('opencode: doctor', () => {
  let dir;
  before(async () => {
    dir = makeTmp();
    await runCli(['init', '--yes', '--providers', 'opencode', '--no-link'], dir);
  });
  after(() => cleanup(dir));

  it('fails and names the agent when its model drifts', async () => {
    const rel = '.opencode/agents/qa-engineer.md';
    writeFile(dir, rel, readFile(dir, rel).replace(/^model:.*$/m, 'model: wrong/model'));
    const { code, stdout, stderr } = await runCli(['doctor'], dir);
    assert.notEqual(code, 0);
    assert.ok((stdout + stderr).includes('qa-engineer'));
  });

  it('fails when mode is not subagent', async () => {
    await runCli(['update', '--yes', '--no-link'], dir);
    const rel = '.opencode/agents/frontend-dev.md';
    writeFile(dir, rel, readFile(dir, rel).replace(/^mode:.*$/m, 'mode: primary'));
    const { code, stdout, stderr } = await runCli(['doctor'], dir);
    assert.notEqual(code, 0);
    assert.ok((stdout + stderr).includes('frontend-dev'));
  });

  it('fails when an agent file is missing', async () => {
    await runCli(['update', '--yes', '--no-link'], dir);
    const { unlinkSync } = await import('node:fs');
    unlinkSync(join(dir, '.opencode/agents/project-manager.md'));
    const { code, stdout, stderr } = await runCli(['doctor'], dir);
    assert.notEqual(code, 0);
    assert.ok((stdout + stderr).includes('project-manager'));
  });
});

describe('opencode: provider flag validation and regression', () => {
  it('unknown provider names are ignored for init as before, valid ones are listed in help', async () => {
    const dir = makeTmp();
    try {
      const { stdout, stderr } = await runCli(['--help'], dir);
      assert.ok((stdout + stderr).includes('claude,antigravity,codex,opencode'));
    } finally { cleanup(dir); }
  });

  it('a workspace without opencode creates no .opencode directory', async () => {
    const dir = makeTmp();
    try {
      await runCli(['init', '--yes', '--providers', 'claude,antigravity,codex', '--no-link'], dir);
      assert.equal(fileExists(dir, '.opencode'), false);
      const { code } = await runCli(['doctor'], dir);
      assert.equal(code, 0);
    } finally { cleanup(dir); }
  });
});

describe('opencode: shipped templates', () => {
  const template = readFileSync(join(ROOT, 'templates/AGENTS.md'), 'utf8');
  const rows = template.split('\n').filter((l) => l.startsWith('|'));
  const header = rows[0].split('|').slice(1, -1).map((c) => c.trim());
  const column = header.findIndex((c) => c.toLowerCase().startsWith('opencode'));

  it('model table has an opencode column that matches models.json', () => {
    assert.ok(column > 0, `header: ${header.join(' | ')}`);
    for (const agent of agents()) {
      const row = rows.find((r) => r.includes(`\`${agent}\``));
      assert.ok(row, `row for ${agent}`);
      const cell = row.split('|').slice(1, -1)[column].trim();
      assert.equal(cell, `\`${resolve(agent, 'opencode')}\``, agent);
    }
  });

  it('keeps the table inside the managed block so update refreshes it', () => {
    const start = template.indexOf('<!-- managed:start -->');
    const end = template.indexOf('<!-- managed:end -->');
    const table = template.indexOf(rows[0]);
    assert.ok(start >= 0 && table > start && table < end);
  });

  it('workspace-sync skill lists opencode', () => {
    const skill = readFileSync(join(ROOT, 'templates/skills/workspace-sync/SKILL.md'), 'utf8');
    assert.ok(/opencode/i.test(skill));
  });

  it('README documents opencode as a provider', () => {
    const readme = readFileSync(join(ROOT, 'README.md'), 'utf8');
    assert.ok(readme.includes('claude,antigravity,codex,opencode') || /--providers[^\n]*opencode/.test(readme));
    assert.ok(/\.opencode\/agents/.test(readme));
  });
});
