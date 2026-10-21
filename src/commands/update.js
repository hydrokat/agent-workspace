import { existsSync, readFileSync } from 'node:fs';
import { fileURLToPath } from 'node:url';
import { dirname, join } from 'node:path';
import { log, configure } from '../core/log.js';
import { gatherUpdateInfo } from '../core/prompts.js';
import { ensureDirs, initStarterFiles } from '../core/scaffold.js';
import { installCanonicalAssets, syncAllProviders, refreshManagedBlock } from '../core/sync.js';
import { installRuntime } from '../core/runtime.js';
import { load as loadManifest, save as saveManifest, bump as bumpManifest } from '../core/manifest.js';
import { repoState, snapshot, changedBetween, commitPaths, currentBranch, isProtectedBranch } from '../core/git.js';
import { confirm, isInteractive } from '../core/prompts.js';
import {
  selectClis, detect, checkLatest, updateCommand, applyUpdate, loadMinVersions, belowMinimum,
} from '../core/ai-clis.js';
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

  const cliResults = await updateAiClis(flags, providers, cwd);
  const applied = cliResults.filter((r) => r.action === 'updated' || r.action === 'current');

  log.summary([
    `Workspace updated: ${cwd}`,
    `Version: ${pkg.version}`,
    `Providers: ${providers.join(', ')}`,
    ok ? '✓ doctor passed' : '⚠ doctor reported issues (see above)',
    ...(applied.length ? [`AI CLIs: ${applied.map(describeResult).join(', ')}`] : []),
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

function describeResult(r) {
  return r.action === 'updated' ? `${r.cli} ${r.before} → ${r.after}` : `${r.cli} up to date`;
}

function minVersionsFile(cwd) {
  const workspace = join(cwd, '.agent-workspace', 'tandem', 'min-versions.env');
  return existsSync(workspace) ? workspace : join(__dirname, '..', '..', 'tandem', 'min-versions.env');
}

function planCli(cli) {
  const info = detect(cli);
  if (!info.installed) return null;
  const latest = checkLatest(info);
  if (latest.state === 'current') return { info, latest, current: true };
  return { info, latest, command: updateCommand(info) };
}

function listLine(plan) {
  const { info, latest, command } = plan;
  const name = info.cli.padEnd(7);
  if (command.skipped) return `↑ ${name} skipped — ${command.skipped}`;
  const found = latest.state === 'available' ? (latest.latest ? `${latest.latest} available` : 'update available') : 'latest unknown';
  const run = command.argv ? command.argv.join(' ') : `run it yourself: ${command.manual}`;
  return `${name} ${(info.version || '?').padEnd(8)} ${info.channel.padEnd(8)} ${found} → ${run}`;
}

/** Checks the workspace's AI CLIs and, with consent, runs each CLI's own update command. Never fails the update. */
async function updateAiClis(flags, providers, cwd) {
  const results = [];
  try {
    const plans = selectClis(providers).map(planCli).filter(Boolean);
    if (plans.length === 0) return results;
    if (flags.updateClis === false) {
      log.info('AI CLI updates skipped (--no-update-clis).');
      return results;
    }
    log.step('AI CLIs');
    for (const plan of plans.filter((p) => p.current)) log.info(`up to date: ${plan.info.cli} ${plan.info.version}`);
    const pending = plans.filter((p) => !p.current);
    for (const plan of pending) log.info(listLine(plan));
    const runnable = pending.filter((p) => p.command.argv);

    let apply = false;
    if (runnable.length > 0) {
      if (flags.dryRun) {
        log.info('(dry run: no CLI was changed)');
      } else if (flags.updateClis === true || flags.yes) {
        apply = true;
      } else if (isInteractive()) {
        apply = await confirm('Check and apply these updates?', false);
      } else {
        log.info(`AI CLIs: ${runnable.length} can be checked; re-run with --update-clis to apply.`);
      }
    }

    for (const plan of runnable) {
      if (!apply) { results.push({ cli: plan.info.cli, action: 'listed', before: plan.info.version }); continue; }
      const r = applyUpdate(plan.info);
      const name = plan.info.cli.padEnd(7);
      if (r.ok) {
        log.info(r.changed ? `✔ ${name} ${r.before} → ${r.after}` : `✔ ${name} ${r.after || r.before} (up to date)`);
        results.push({ cli: plan.info.cli, action: r.changed ? 'updated' : 'current', before: r.before, after: r.after });
      } else {
        const why = r.timedOut ? 'timed out' : `exit ${r.exitCode ?? '?'}`;
        log.info(`✖ ${name} update failed (${why}) — run it yourself: ${r.command}`);
        results.push({ cli: plan.info.cli, action: 'failed', before: r.before, after: r.after, command: r.command });
      }
    }
    for (const plan of pending.filter((p) => !p.command.argv)) {
      results.push({ cli: plan.info.cli, action: plan.command.skipped ? 'skipped' : 'manual', before: plan.info.version });
    }

    const mins = loadMinVersions(minVersionsFile(cwd));
    for (const plan of plans) {
      const now = apply && !flags.dryRun ? detect(plan.info.cli) : plan.info;
      if (belowMinimum(now, mins)) {
        const fix = plan.command?.argv ? plan.command.argv.join(' ') : `see ${plan.info.cli}'s documentation`;
        log.warn(`${now.cli} ${now.version} is below tandem's minimum ${mins[`TANDEM_MIN_${now.cli.toUpperCase()}`]} — run: ${fix}`);
      }
    }
    log.data('aiClis', results);
  } catch (e) {
    log.warn(`Could not check AI CLIs: ${e.message}`);
  }
  return results;
}
