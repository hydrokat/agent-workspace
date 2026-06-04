import { readFileSync } from 'node:fs';
import { fileURLToPath } from 'node:url';
import { dirname, join } from 'node:path';
import { log, configure } from '../core/log.js';
import { gatherUpdateInfo } from '../core/prompts.js';
import { ensureDirs, initStarterFiles } from '../core/scaffold.js';
import { installCanonicalAssets, syncAllProviders, refreshManagedBlock } from '../core/sync.js';
import { load as loadManifest, save as saveManifest, bump as bumpManifest } from '../core/manifest.js';
import { run as runDoctor } from './doctor.js';

const __dirname = dirname(fileURLToPath(import.meta.url));

export async function run({ flags, cwd }) {
  configure({ quiet: flags.quiet, verbose: flags.verbose, json: flags.json });

  const existing = loadManifest(cwd);
  const { providers } = await gatherUpdateInfo(flags, existing);

  const pkg = JSON.parse(readFileSync(join(__dirname, '..', '..', 'package.json'), 'utf8'));

  let manifest;
  if (existing) {
    manifest = { ...existing, providers };
  } else {
    manifest = { version: pkg.version, createdAt: new Date().toISOString(), updatedAt: new Date().toISOString(), projectName: '', providers, managedFiles: [] };
  }

  log.step('Ensuring directory structure');
  if (!flags.dryRun) ensureDirs(cwd);
  initStarterFiles(cwd, flags.dryRun);

  log.step('Refreshing AGENTS.md managed block');
  refreshManagedBlock(cwd, flags.dryRun);

  installCanonicalAssets(cwd, manifest, flags.dryRun);

  syncAllProviders(cwd, providers, manifest, flags.dryRun);

  if (!flags.dryRun) {
    const updated = bumpManifest(manifest, pkg.version);
    saveManifest(cwd, updated);
  }

  log.step('Verifying workspace');
  const ok = await runDoctor({ flags: { ...flags, _internal: true }, cwd });

  log.summary([
    `Workspace updated: ${cwd}`,
    `Version: ${pkg.version}`,
    `Providers: ${providers.join(', ')}`,
    ok ? '✓ doctor passed' : '⚠ doctor reported issues (see above)',
  ]);
}
