import { readFileSync } from 'node:fs';
import { fileURLToPath } from 'node:url';
import { dirname, join } from 'node:path';
import { log, configure } from '../core/log.js';
import { gatherUpdateInfo } from '../core/prompts.js';
import { ensureDirs, initStarterFiles } from '../core/scaffold.js';
import { installCanonicalAssets, syncAllProviders, refreshManagedBlock } from '../core/sync.js';
import { installRuntime } from '../core/runtime.js';
import { load as loadManifest, save as saveManifest, bump as bumpManifest } from '../core/manifest.js';
import { repoState, snapshot, changedBetween, commitPaths, currentBranch, isProtectedBranch } from '../core/git.js';
import { confirm } from '../core/prompts.js';
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

  const ownRepo = repoState(cwd) === 'own';
  const before = ownRepo ? snapshot(cwd) : null;
  if (ownRepo && before.size > 0) {
    log.info(`Workspace has ${before.size} uncommitted change(s) already; only files this update touches will be offered for commit.`);
  }

  log.step('Ensuring directory structure');
  if (!flags.dryRun) ensureDirs(cwd);
  initStarterFiles(cwd, flags.dryRun);

  log.step('Refreshing AGENTS.md managed block');
  refreshManagedBlock(cwd, flags.dryRun);

  installCanonicalAssets(cwd, manifest, flags.dryRun);

  installRuntime(cwd, manifest, flags.dryRun);

  syncAllProviders(cwd, providers, manifest, flags.dryRun);

  if (!flags.dryRun) {
    const updated = bumpManifest(manifest, pkg.version);
    saveManifest(cwd, updated);
  }

  if (ownRepo && !flags.dryRun) await offerCommit(flags, cwd, before, pkg.version);
  else if (!ownRepo) log.info('This workspace is not its own git repository; skipping the commit step (`init --git` creates one).');

  log.step('Verifying workspace');
  const ok = await runDoctor({ flags: { ...flags, _internal: true }, cwd });

  log.summary([
    `Workspace updated: ${cwd}`,
    `Version: ${pkg.version}`,
    `Providers: ${providers.join(', ')}`,
    ok ? '✓ doctor passed' : '⚠ doctor reported issues (see above)',
  ]);
}

async function offerCommit(flags, cwd, before, version) {
  const changed = changedBetween(before, snapshot(cwd));
  if (changed.length === 0) return;
  if (flags.commit === false) return;
  let yes = flags.commit === true;
  if (!yes && !flags.yes) {
    const branch = currentBranch(cwd);
    const where = branch ? ` on ${isProtectedBranch(branch) ? `protected branch '${branch}'` : `'${branch}'`}` : '';
    yes = await confirm(`Commit the ${changed.length} file(s) changed by this update${where}?`, false);
  }
  if (!yes) return;
  try {
    commitPaths(cwd, changed, `chore: update agent-workspace to ${version}`);
    log.created(`commit: chore: update agent-workspace to ${version} (${changed.length} files)`);
  } catch (e) {
    log.warn(`Could not commit: ${e.message}`);
  }
}
