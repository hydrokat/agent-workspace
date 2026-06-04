import { existsSync, readFileSync, lstatSync, readdirSync } from 'node:fs';
import { join } from 'node:path';
import { parse as parseFm } from '../core/frontmatter.js';
import { resolve as resolveModel, agents as allAgents } from '../core/models.js';
import { getProvider } from '../core/providers.js';
import { load as loadManifest } from '../core/manifest.js';
import { log, configure } from '../core/log.js';
import { listAgents } from '../core/assets.js';

function check(passed, label) {
  if (passed) log.ok(label);
  else log.error(label);
  return passed;
}

export async function run({ flags, cwd }) {
  if (!flags._internal) {
    configure({ quiet: flags.quiet, verbose: flags.verbose, json: flags.json });
    log.step('Doctor — workspace verification');
  }

  const manifest = loadManifest(cwd);
  const providers = manifest?.providers ?? ['claude'];
  const results = [];

  for (const dir of ['.agents/agents', '.agents/skills', 'knowledgebase', 'specs']) {
    results.push(check(existsSync(join(cwd, dir)), `Dir exists: ${dir}`));
  }
  results.push(check(existsSync(join(cwd, 'AGENTS.md')), 'AGENTS.md present'));

  for (const providerName of providers) {
    const provider = getProvider(providerName);
    if (!provider) {
      results.push(check(false, `Provider "${providerName}" is recognized`));
      continue;
    }

    const agentsDir = join(cwd, provider.agentsDir);

    for (const agentName of listAgents()) {
      const agentPath = provider.native
        ? join(agentsDir, agentName, 'AGENT.md')
        : join(agentsDir, `${agentName}.md`);

      const exists = existsSync(agentPath);
      results.push(check(exists, `[${providerName}] agent exists: ${agentName}`));

      if (exists) {
        const content = readFileSync(agentPath, 'utf8');
        const { frontmatter } = parseFm(content);
        results.push(check(!!frontmatter.name, `[${providerName}] ${agentName}: has name`));
        results.push(check(!!frontmatter.description, `[${providerName}] ${agentName}: has description`));
        results.push(check(!!frontmatter.model, `[${providerName}] ${agentName}: has model`));

        const expectedModel = resolveModel(agentName, providerName);
        if (expectedModel && frontmatter.model) {
          results.push(check(
            frontmatter.model === expectedModel,
            `[${providerName}] ${agentName}: model matches models.json (${expectedModel})`
          ));
        }
      }
    }

    if (!provider.native && provider.doc !== 'AGENTS.md') {
      const docPath = join(cwd, provider.doc);
      let docOk = false;
      try {
        const stat = lstatSync(docPath);
        if (stat.isSymbolicLink()) {
          docOk = true;
        } else if (!existsSync(docPath)) {
          docOk = true;
        }
      } catch {
        docOk = true;
      }
      results.push(check(docOk, `[${providerName}] compat doc is symlink or absent: ${provider.doc}`));
    }
  }

  const knownAgents = allAgents();
  const templateAgents = listAgents();
  for (const agent of templateAgents) {
    results.push(check(knownAgents.includes(agent), `models.json covers agent: ${agent}`));
  }

  const symlinkRoot = join(cwd, 'codebase-symlinks');
  if (existsSync(symlinkRoot)) {
    for (const name of readdirSync(symlinkRoot)) {
      const p = join(symlinkRoot, name);
      let resolves = false;
      try { lstatSync(p); resolves = existsSync(p); } catch { /* */ }
      results.push(check(resolves, `codebase-symlinks/${name} resolves`));
    }
  }

  const passed = results.every(Boolean);
  const passCount = results.filter(Boolean).length;

  if (!flags._internal) {
    log.summary([
      `${passCount}/${results.length} checks passed`,
      passed ? '✓ Workspace is healthy' : '✗ Workspace has issues',
    ]);
  }

  return passed;
}
