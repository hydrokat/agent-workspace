import { mkdirSync, existsSync, writeFileSync } from 'node:fs';
import { join } from 'node:path';
import { log } from './log.js';
import { readTemplate } from './assets.js';

export function ensureDir(target, rel) {
  const full = join(target, rel);
  mkdirSync(full, { recursive: true });
  return full;
}

export function ensureDirs(target) {
  const dirs = [
    '.agents/agents',
    '.agents/skills',
    'knowledgebase/guidelines',
    'knowledgebase/best-practices',
    'knowledgebase/business-flows',
    'knowledgebase/context-history',
    'specs',
    '.agent-workspace',
    '.tmp',
  ];
  for (const dir of dirs) ensureDir(target, dir);
}

export function createIfMissing(target, rel, content, dryRun = false) {
  const full = join(target, rel);
  if (existsSync(full)) {
    log.unchanged(rel);
    return false;
  }
  if (!dryRun) {
    mkdirSync(join(target, rel.split('/').slice(0, -1).join('/')), { recursive: true });
    writeFileSync(full, content, 'utf8');
  }
  log.created(rel);
  return true;
}

export function initStarterFiles(target, dryRun = false) {
  const starters = [
    ['AGENTS.md', readTemplate('AGENTS.md')],
    ['WORKFLOW.md', readTemplate('WORKFLOW.md')],
    ['knowledgebase/README.md', readTemplate('knowledgebase', 'README.md')],
    ['knowledgebase/guidelines/README.md', readTemplate('knowledgebase', 'guidelines', 'README.md')],
    ['knowledgebase/best-practices/README.md', readTemplate('knowledgebase', 'best-practices', 'README.md')],
    ['knowledgebase/business-flows/README.md', readTemplate('knowledgebase', 'business-flows', 'README.md')],
    ['knowledgebase/context-history/README.md', readTemplate('knowledgebase', 'context-history', 'README.md')],
    ['specs/README.md', readTemplate('specs', 'README.md')],
  ];
  for (const [rel, content] of starters) {
    createIfMissing(target, rel, content, dryRun);
  }
}
