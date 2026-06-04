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
import { linkCodebases } from '../core/link-codebases.js';
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

  const { projectName, providers, linkCodebases: doLink, linkAll } = await gatherInitInfo(flags);

  const pkg = JSON.parse(readFileSync(join(__dirname, '..', '..', 'package.json'), 'utf8'));
  const manifest = createManifest({ projectName, providers, version: pkg.version });

  log.step('Creating directory structure');
  if (!flags.dryRun) ensureDirs(cwd);

  log.step('Creating starter files');
  initStarterFiles(cwd, flags.dryRun);

  installCanonicalAssets(cwd, manifest, flags.dryRun);

  syncAllProviders(cwd, providers, manifest, flags.dryRun);

  if (doLink) {
    log.step('Linking sibling codebases');
    await linkCodebases(cwd, { linkAll, dryRun: flags.dryRun });
  }

  if (!flags.dryRun) saveManifest(cwd, manifest);

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
