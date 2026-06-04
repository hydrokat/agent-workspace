import { readFileSync, writeFileSync, existsSync, mkdirSync } from 'node:fs';
import { join } from 'node:path';

const MANIFEST_REL = '.agent-workspace/manifest.json';

export function manifestPath(target) {
  return join(target, MANIFEST_REL);
}

export function load(target) {
  const p = manifestPath(target);
  if (!existsSync(p)) return null;
  try {
    return JSON.parse(readFileSync(p, 'utf8'));
  } catch {
    return null;
  }
}

export function save(target, data) {
  const p = manifestPath(target);
  mkdirSync(join(target, '.agent-workspace'), { recursive: true });
  writeFileSync(p, JSON.stringify(data, null, 2) + '\n', 'utf8');
}

export function create({ projectName, providers, version }) {
  const now = new Date().toISOString();
  return {
    version,
    createdAt: now,
    updatedAt: now,
    projectName: projectName ?? '',
    providers,
    managedFiles: [],
  };
}

export function bump(manifest, version) {
  return { ...manifest, version, updatedAt: new Date().toISOString() };
}

export function addManagedFile(manifest, rel) {
  if (!manifest.managedFiles.includes(rel)) {
    manifest.managedFiles.push(rel);
  }
}

export function writeManaged(target, rel, content, manifest, dryRun = false) {
  const full = join(target, rel);
  let action = 'created';

  if (existsSync(full)) {
    const existing = readFileSync(full, 'utf8');
    if (existing === content) {
      addManagedFile(manifest, rel);
      return 'unchanged';
    }
    if (!dryRun) {
      writeFileSync(full + '.bak', existing, 'utf8');
    }
    action = 'updated';
  }

  if (!dryRun) {
    mkdirSync(join(target, rel.split('/').slice(0, -1).join('/')), { recursive: true });
    writeFileSync(full, content, 'utf8');
  }

  addManagedFile(manifest, rel);
  return action;
}
