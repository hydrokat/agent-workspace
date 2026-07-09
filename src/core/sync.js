import { readdirSync, readFileSync, writeFileSync, copyFileSync, mkdirSync, existsSync, lstatSync, symlinkSync, readlinkSync, unlinkSync, statSync, rmSync } from 'node:fs';
import { join, relative } from 'node:path';
import { parse as parseFm, serialize as serializeFm } from './frontmatter.js';
import { log } from './log.js';
import { getProvider } from './providers.js';
import { templatePath, listAgents, listSkills } from './assets.js';

function writeManaged(full, content, dryRun) {
  let action = 'created';
  if (existsSync(full)) {
    const existing = readFileSync(full, 'utf8');
    if (existing === content) return 'unchanged';
    if (!dryRun) writeFileSync(full + '.bak', existing, 'utf8');
    action = 'updated';
  }
  if (!dryRun) {
    mkdirSync(join(full, '..'), { recursive: true });
    writeFileSync(full, content, 'utf8');
  }
  return action;
}

function syncDocLink(target, providerDoc, dryRun) {
  if (providerDoc === 'AGENTS.md') return;
  const docPath = join(target, providerDoc);

  if (existsSync(docPath) || (lstatSync(docPath).isSymbolicLink ? false : false)) {
    try {
      const stat = lstatSync(docPath);
      if (stat.isSymbolicLink()) {
        const dest = readlinkSync(docPath);
        if (dest === 'AGENTS.md' || dest === './AGENTS.md') return;
        log.warn(`${providerDoc} is an unmanaged symlink → skipping`);
        return;
      }
      log.skipped(`${providerDoc} (existing file, not overwriting)`);
      return;
    } catch { /* absent */ }
  }

  if (!dryRun) symlinkSync('AGENTS.md', docPath);
  log.created(`${providerDoc} → AGENTS.md`);
}

function syncDocLinkSafe(target, providerDoc, dryRun) {
  if (providerDoc === 'AGENTS.md') return;
  const docPath = join(target, providerDoc);
  let exists = false;
  let stat;
  try { stat = lstatSync(docPath); exists = true; } catch { exists = false; }

  if (exists) {
    if (stat.isSymbolicLink()) {
      const dest = readlinkSync(docPath);
      if (dest === 'AGENTS.md' || dest === './AGENTS.md') return;
      log.warn(`${providerDoc} is an unmanaged symlink — skipping`);
      return;
    }
    log.skipped(`${providerDoc} (existing file, not overwriting)`);
    return;
  }

  if (!dryRun) symlinkSync('AGENTS.md', docPath);
  log.created(`${providerDoc} → AGENTS.md`);
}

function clearDir(dir) {
  if (!existsSync(dir)) return;
  for (const entry of readdirSync(dir)) {
    const p = join(dir, entry);
    const s = lstatSync(p);
    if (s.isDirectory()) rmSync(p, { recursive: true });
    else unlinkSync(p);
  }
}

function copyDirRecursive(src, dest, dryRun) {
  if (!dryRun) mkdirSync(dest, { recursive: true });
  for (const entry of readdirSync(src)) {
    const srcPath = join(src, entry);
    const destPath = join(dest, entry);
    const s = statSync(srcPath);
    if (s.isDirectory()) {
      copyDirRecursive(srcPath, destPath, dryRun);
    } else {
      const content = readFileSync(srcPath, 'utf8');
      const action = writeManaged(destPath, content, dryRun);
      if (action === 'created') log.created(destPath);
      else if (action === 'updated') { log.backedUp(destPath); log.updated(destPath); }
      else log.unchanged(destPath);
    }
  }
}

export function syncProvider(target, providerName, manifest, dryRun = false) {
  const provider = getProvider(providerName);
  if (!provider) throw new Error(`Unknown provider: ${providerName}`);

  log.step(`Syncing ${providerName}`);

  syncDocLinkSafe(target, provider.doc, dryRun);

  const agentsDir = join(target, provider.agentsDir);
  if (!dryRun) mkdirSync(agentsDir, { recursive: true });

  const writtenAgentFiles = new Set();

  for (const agentName of listAgents()) {
    const tmplPath = templatePath('agents', agentName, 'AGENT.md');
    const raw = readFileSync(tmplPath, 'utf8');
    const { frontmatter, body } = parseFm(raw);
    const transformed = provider.transformAgent(frontmatter, agentName);
    const outContent = serializeFm(transformed, body);

    const outPath = provider.native
      ? join(agentsDir, agentName, 'AGENT.md')
      : join(agentsDir, `${agentName}.md`);

    writtenAgentFiles.add(outPath);
    if (!dryRun) mkdirSync(join(outPath, '..'), { recursive: true });
    const action = writeManaged(outPath, outContent, dryRun);
    const rel = relative(target, outPath);
    if (manifest) {
      if (!manifest.managedFiles.includes(rel)) manifest.managedFiles.push(rel);
    }
    if (action === 'created') log.created(rel);
    else if (action === 'updated') { log.backedUp(rel + '.bak'); log.updated(rel); }
    else log.unchanged(rel);
  }

  // Remove stale agent files (present in dir but not in current template); skip .bak files
  if (!provider.native && !dryRun && existsSync(agentsDir)) {
    for (const entry of readdirSync(agentsDir)) {
      if (entry.endsWith('.bak')) continue;
      const p = join(agentsDir, entry);
      if (!writtenAgentFiles.has(p)) {
        unlinkSync(p);
        log.info(`Removed stale agent: ${relative(target, p)}`);
      }
    }
  }

  if (!provider.nativeSkills) {
    const skillsDir = join(target, provider.skillsDir);
    if (!dryRun) mkdirSync(skillsDir, { recursive: true });

    const writtenSkillDirs = new Set();
    for (const skillName of listSkills()) {
      const srcDir = templatePath('skills', skillName);
      const destDir = join(skillsDir, skillName);
      writtenSkillDirs.add(skillName);
      copyDirRecursive(srcDir, destDir, dryRun);
      if (manifest) {
        const rel = relative(target, destDir);
        if (!manifest.managedFiles.includes(rel)) manifest.managedFiles.push(rel);
      }
    }

    if (!dryRun && existsSync(skillsDir)) {
      for (const entry of readdirSync(skillsDir)) {
        if (!writtenSkillDirs.has(entry)) {
          rmSync(join(skillsDir, entry), { recursive: true });
          log.info(`Removed stale skill: ${relative(target, join(skillsDir, entry))}`);
        }
      }
    }
  }
}

export function syncAllProviders(target, providers, manifest, dryRun = false) {
  for (const p of providers) syncProvider(target, p, manifest, dryRun);
}

export function installCanonicalAssets(target, manifest, dryRun = false) {
  log.step('Installing agents');
  for (const agentName of listAgents()) {
    const srcPath = templatePath('agents', agentName, 'AGENT.md');
    const destDir = join(target, '.agents', 'agents', agentName);
    const destPath = join(destDir, 'AGENT.md');
    if (!dryRun) mkdirSync(destDir, { recursive: true });
    const content = readFileSync(srcPath, 'utf8');
    const action = writeManaged(destPath, content, dryRun);
    const rel = relative(target, destPath);
    if (manifest) { if (!manifest.managedFiles.includes(rel)) manifest.managedFiles.push(rel); }
    if (action === 'created') log.created(rel);
    else if (action === 'updated') { log.backedUp(rel + '.bak'); log.updated(rel); }
    else log.unchanged(rel);
  }

  log.step('Installing skills');
  for (const skillName of listSkills()) {
    const srcDir = templatePath('skills', skillName);
    const destDir = join(target, '.agents', 'skills', skillName);
    copyDirRecursive(srcDir, destDir, dryRun);
    if (manifest) {
      const rel = relative(target, destDir);
      if (!manifest.managedFiles.includes(rel)) manifest.managedFiles.push(rel);
    }
  }

  log.step('Installing knowledgebase assets');
  for (const subdir of readdirSync(templatePath('knowledgebase'))) {
    const subdirPath = templatePath('knowledgebase', subdir);
    if (!statSync(subdirPath).isDirectory()) continue;

    const isGuidelines = subdir === 'guidelines';
    for (const f of readdirSync(subdirPath)) {
      if (!f.endsWith('.md')) continue;
      if (f === 'README.md' && !isGuidelines) continue;
      const src = join(subdirPath, f);
      const dest = join(target, 'knowledgebase', subdir, f);
      if (!dryRun) mkdirSync(join(dest, '..'), { recursive: true });
      const content = readFileSync(src, 'utf8');
      const action = writeManaged(dest, content, dryRun);
      const rel = relative(target, dest);
      if (manifest) { if (!manifest.managedFiles.includes(rel)) manifest.managedFiles.push(rel); }
      if (action === 'created') log.created(rel);
      else if (action === 'updated') { log.backedUp(rel + '.bak'); log.updated(rel); }
      else log.unchanged(rel);
    }
  }
}

const MANAGED_START = '<!-- managed:start -->';
const MANAGED_END = '<!-- managed:end -->';

export function refreshManagedBlock(target, dryRun = false) {
  const agentsMdPath = join(target, 'AGENTS.md');
  if (!existsSync(agentsMdPath)) return false;

  const existing = readFileSync(agentsMdPath, 'utf8');
  const templateContent = readFileSync(templatePath('AGENTS.md'), 'utf8');

  const startIdx = templateContent.indexOf(MANAGED_START);
  const endIdx = templateContent.indexOf(MANAGED_END);
  if (startIdx === -1 || endIdx === -1) return false;
  const newBlock = templateContent.slice(startIdx, endIdx + MANAGED_END.length);

  const existStart = existing.indexOf(MANAGED_START);
  const existEnd = existing.indexOf(MANAGED_END);
  if (existStart === -1 || existEnd === -1) {
    log.warn('AGENTS.md has no managed block markers — skipping managed block refresh');
    return false;
  }

  const before = existing.slice(0, existStart);
  const after = existing.slice(existEnd + MANAGED_END.length);
  const updated = before + newBlock + after;

  if (updated === existing) {
    log.unchanged('AGENTS.md (managed block)');
    return false;
  }

  if (!dryRun) {
    writeFileSync(agentsMdPath + '.bak', existing, 'utf8');
    writeFileSync(agentsMdPath, updated, 'utf8');
  }
  log.updated('AGENTS.md (managed block)');
  return true;
}
