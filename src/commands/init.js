import { existsSync } from 'node:fs';
import { join } from 'node:path';
import { readFileSync } from 'node:fs';
import { fileURLToPath } from 'node:url';
import { dirname } from 'node:path';
import { log, configure } from '../core/log.js';
import { gatherInitInfo } from '../core/prompts.js';
import { ensureDirs, initStarterFiles } from '../core/scaffold.js';
import { installCanonicalAssets, syncAllProviders } from '../core/sync.js';
import { load as loadManifest, save as saveManifest, create as createManifest } from '../core/manifest.js';
import { installRuntime } from '../core/runtime.js';
import { linkCodebases } from '../core/link-codebases.js';
import { repoState, initRepo, gitAvailable } from '../core/git.js';
import { confirm } from '../core/prompts.js';
import { run as runDoctor } from './doctor.js';

const __dirname = dirname(fileURLToPath(import.meta.url));

export async function run({ flags, cwd }) {
  configure({ quiet: flags.quiet, verbose: flags.verbose, json: flags.json });

  const existing = loadManifest(cwd);
  if (existing && !flags.yes && !flags.force) {
    log.warn('This directory already has an agent-workspace manifest.');
    log.info('Run `agent-workspace update` to refresh, or use --force to re-initialize.');
    return;
  }

  const wantsRepo = await decideRepo(flags, cwd);

  const { projectName, providers, linkCodebases: doLink, linkAll } = await gatherInitInfo(flags);

  const pkg = JSON.parse(readFileSync(join(__dirname, '..', '..', 'package.json'), 'utf8'));
  const manifest = createManifest({ projectName, providers, version: pkg.version });

  log.step('Creating directory structure');
  if (!flags.dryRun) ensureDirs(cwd);

  log.step('Creating starter files');
  initStarterFiles(cwd, flags.dryRun);

  installCanonicalAssets(cwd, manifest, flags.dryRun);

  installRuntime(cwd, manifest, flags.dryRun);

  syncAllProviders(cwd, providers, manifest, flags.dryRun);

  if (doLink) {
    log.step('Linking sibling codebases');
    await linkCodebases(cwd, { linkAll, dryRun: flags.dryRun });
  }

  if (!flags.dryRun) saveManifest(cwd, manifest);

  if (wantsRepo) {
    log.step('Creating a local git repository');
    if (!flags.dryRun) initRepo(cwd);
    log.created('.git (blank local repository; link a remote later with `git remote add origin <url>`)');
  }

  log.step('Verifying workspace');
  const ok = await runDoctor({ flags: { ...flags, _internal: true }, cwd });

  const lines = [
    `Workspace initialized: ${cwd}`,
    `Project: ${projectName || '(unnamed)'}`,
    `Providers: ${providers.join(', ')}`,
    `Agents: ${manifest.managedFiles.filter(f => f.includes('/agents/')).length} files`,
    `Skills: ${manifest.managedFiles.filter(f => f.includes('/skills/')).length} directories`,
    ok ? '✓ doctor passed' : '⚠ doctor reported issues (see above)',
    '',
    'Next steps:',
    '  • Edit AGENTS.md → set your project overview',
    '  • Run `agent-workspace update` after changing canonical files',
  ];
  log.summary(lines);
}

async function decideRepo(flags, cwd) {
  const state = repoState(cwd);
  if (state === 'own') return false;
  if (state === 'nested') {
    log.info('This folder is inside another git repository; not creating a nested one.');
    return false;
  }
  if (!gitAvailable()) {
    if (flags.git) log.warn('git was not found; skipping repository creation.');
    return false;
  }
  if (flags.git !== null) return flags.git;
  if (flags.yes) return false;
  return confirm('This folder is not a git repository. Create a blank local one? (you can add a remote later)', false);
}
