import { readdirSync, readFileSync, statSync } from 'node:fs';
import { fileURLToPath } from 'node:url';
import { dirname, join } from 'node:path';

const TEMPLATES_DIR = join(dirname(fileURLToPath(import.meta.url)), '..', '..', 'templates');

export function templatesDir() {
  return TEMPLATES_DIR;
}

export function templatePath(...parts) {
  return join(TEMPLATES_DIR, ...parts);
}

export function readTemplate(...parts) {
  return readFileSync(templatePath(...parts), 'utf8');
}

export function listAgents() {
  const dir = templatePath('agents');
  return readdirSync(dir).filter(name => statSync(join(dir, name)).isDirectory());
}

export function listSkills() {
  const dir = templatePath('skills');
  return readdirSync(dir).filter(name => statSync(join(dir, name)).isDirectory());
}

export function listGuidelineFiles() {
  return readdirSync(templatePath('knowledgebase', 'guidelines'))
    .filter(f => f.endsWith('.md'));
}

export function listBestPracticeFiles() {
  return readdirSync(templatePath('knowledgebase', 'best-practices'))
    .filter(f => f.endsWith('.md'));
}
