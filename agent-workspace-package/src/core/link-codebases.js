import { readdirSync, readFileSync, writeFileSync, mkdirSync, existsSync, lstatSync, symlinkSync, realpathSync, statSync } from 'node:fs';
import { join, relative, resolve as resolvePath, basename } from 'node:path';
import { createInterface } from 'node:readline/promises';

const CELL_LIMIT = 120;

function normalizeText(val, limit) {
  const cleaned = val.replace(/\s+/g, ' ').replace(/\|/g, '&#124;').trim();
  if (limit && cleaned.length > limit) return cleaned.slice(0, limit - 3).trimEnd() + '...';
  return cleaned;
}

function readText(path) {
  try { return readFileSync(path, 'utf8'); } catch { return ''; }
}

function extractReadmeSummary(dir) {
  const text = readText(join(dir, 'README.md'));
  if (!text) return '';
  const lines = text.split('\n');
  const para = [];
  let sawHeading = false;
  for (const raw of lines) {
    const line = raw.trim();
    if (!line) { if (para.length) break; continue; }
    if (line.startsWith('#')) { sawHeading = true; continue; }
    if (sawHeading) para.push(line);
  }
  if (para.length) return normalizeText(para.join(' '), CELL_LIMIT);
  const headings = lines.filter(l => l.trim().startsWith('#')).map(l => l.replace(/^#+\s*/, '').trim());
  for (const h of headings) if (h) return normalizeText(h, CELL_LIMIT);
  const nonempty = lines.map(l => l.trim()).filter(Boolean);
  return nonempty[0] ? normalizeText(nonempty[0], CELL_LIMIT) : '';
}

function extractJsonDescription(dir) {
  try {
    const data = JSON.parse(readText(join(dir, 'package.json')));
    for (const key of ['description', 'name']) {
      if (typeof data[key] === 'string' && data[key].trim()) return normalizeText(data[key].trim(), CELL_LIMIT);
    }
  } catch { /* */ }
  return '';
}

function extractTomlField(dir, file, field) {
  const text = readText(join(dir, file));
  const pattern = new RegExp(`^\\s*${field}\\s*=\\s*["'](.+?)["']\\s*$`, 'm');
  const m = text.match(pattern);
  return m ? normalizeText(m[1].trim(), CELL_LIMIT) : '';
}

function summarize(dir) {
  return extractReadmeSummary(dir)
    || extractJsonDescription(dir)
    || extractTomlField(dir, 'pyproject.toml', 'description')
    || extractTomlField(dir, 'pyproject.toml', 'name')
    || extractTomlField(dir, 'Cargo.toml', 'description')
    || extractTomlField(dir, 'Cargo.toml', 'name')
    || normalizeText(basename(dir), CELL_LIMIT);
}

function discoverSiblings(workspaceRoot) {
  const parent = join(workspaceRoot, '..');
  const resolvedRoot = resolvePath(workspaceRoot);
  const results = [];
  try {
    for (const name of readdirSync(parent).sort((a, b) => a.toLowerCase().localeCompare(b.toLowerCase()))) {
      const full = resolvePath(join(parent, name));
      try {
        if (!statSync(full).isDirectory()) continue;
        if (full === resolvedRoot) continue;
        results.push({ name, path: full, summary: summarize(full) });
      } catch { /* */ }
    }
  } catch { /* */ }
  return results;
}

async function promptSelection(candidates) {
  const iface = createInterface({ input: process.stdin, output: process.stdout });
  try {
    console.log('\nAvailable sibling codebases:');
    candidates.forEach((c, i) => {
      console.log(`  ${i + 1}. ${c.name}`);
      console.log(`     ${c.summary}`);
    });
    const raw = (await iface.question('\nSelect (all, comma-separated numbers, or q to skip): ')).trim().toLowerCase();
    if (!raw || raw === 'q' || raw === 'quit') return [];
    if (raw === 'all' || raw === 'a' || raw === '*') return candidates;
    const parts = raw.split(/[\s,]+/).filter(Boolean);
    const selected = [];
    for (const p of parts) {
      const idx = parseInt(p, 10);
      if (!isNaN(idx) && idx >= 1 && idx <= candidates.length) selected.push(candidates[idx - 1]);
    }
    return [...new Map(selected.map(c => [c.name, c])).values()];
  } finally {
    iface.close();
  }
}

function linkCodebase(source, dest) {
  mkdirSync(join(dest, '..'), { recursive: true });
  let existsStat;
  try { existsStat = lstatSync(dest); } catch { existsStat = null; }

  if (existsStat) {
    if (existsStat.isSymbolicLink()) {
      try {
        if (realpathSync(dest) === source) return 'preserved';
      } catch { /* */ }
    }
    return 'skipped';
  }

  const rel = relative(join(dest, '..'), source);
  symlinkSync(rel, dest);
  return 'created';
}

function renderMap(entries, workspaceRoot, symlinkRoot) {
  const relRoot = relative(workspaceRoot, symlinkRoot);
  const lines = [
    '# Codebase Map',
    '',
    'This note records sibling repositories linked into `codebase-symlinks/` so agents can identify the right codebase before traversing back to the parent project.',
    '',
    `- Symlink root: \`${relRoot}\``,
    '',
    '| Codebase | Symlink | Purpose |',
    '| --- | --- | --- |',
  ];

  if (!entries.length) {
    lines.push('| _None_ | _None_ | No linked codebases recorded yet. |');
  } else {
    for (const e of entries) {
      const symRel = relative(workspaceRoot, e.symlink);
      lines.push(`| \`${e.name}\` | \`${symRel}\` | ${e.summary} |`);
    }
  }

  lines.push('', 'Keep the purpose line short and specific when the linked repos change.');
  return lines.join('\n') + '\n';
}

function refreshMap(workspaceRoot, symlinkRoot) {
  const mapPath = join(workspaceRoot, 'knowledgebase', 'context-history', 'codebase-map.md');
  mkdirSync(join(mapPath, '..'), { recursive: true });

  const entries = [];
  if (existsSync(symlinkRoot)) {
    for (const name of readdirSync(symlinkRoot).sort()) {
      const p = join(symlinkRoot, name);
      try {
        if (!lstatSync(p).isSymbolicLink()) continue;
        const source = resolvePath(p);
        entries.push({ name, symlink: p, summary: summarize(source) });
      } catch { /* */ }
    }
  }

  writeFileSync(mapPath, renderMap(entries, workspaceRoot, symlinkRoot), 'utf8');
  return mapPath;
}

export async function linkCodebases(workspaceRoot, { linkAll = false, dryRun = false } = {}) {
  const symlinkRoot = join(workspaceRoot, 'codebase-symlinks');
  const candidates = discoverSiblings(workspaceRoot);

  if (!candidates.length) {
    console.log('No sibling directories found.');
    return { created: 0, preserved: 0, skipped: 0 };
  }

  let selected;
  if (linkAll) {
    selected = candidates;
  } else {
    try {
      selected = await promptSelection(candidates);
    } catch {
      selected = [];
    }
  }

  if (!selected.length) {
    if (!dryRun) refreshMap(workspaceRoot, symlinkRoot);
    return { created: 0, preserved: 0, skipped: 0 };
  }

  const counts = { created: 0, preserved: 0, skipped: 0 };
  if (!dryRun) mkdirSync(symlinkRoot, { recursive: true });

  for (const c of selected) {
    const dest = join(symlinkRoot, c.name);
    const status = dryRun ? 'created' : linkCodebase(c.path, dest);
    counts[status] = (counts[status] ?? 0) + 1;
    console.log(`  ${status === 'created' ? '+' : status === 'preserved' ? '=' : '↷'} ${c.name}`);
  }

  if (!dryRun) {
    const mapPath = refreshMap(workspaceRoot, symlinkRoot);
    console.log(`  Updated codebase map: ${relative(workspaceRoot, mapPath)}`);
  }

  return counts;
}
