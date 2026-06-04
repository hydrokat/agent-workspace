import { readFileSync } from 'node:fs';
import { fileURLToPath } from 'node:url';
import { dirname, join } from 'node:path';

const MODELS_PATH = join(dirname(fileURLToPath(import.meta.url)), '..', 'data', 'models.json');
let _map = null;

function load() {
  if (!_map) _map = JSON.parse(readFileSync(MODELS_PATH, 'utf8'));
  return _map;
}

export function resolve(agent, provider) {
  const map = load();
  return map[agent]?.[provider] ?? null;
}

export function agents() {
  return Object.keys(load());
}

export function providersFor(agent) {
  const map = load();
  return Object.keys(map[agent] ?? {});
}

export function validate(selectedProviders) {
  const map = load();
  const errors = [];
  for (const agent of Object.keys(map)) {
    for (const provider of selectedProviders) {
      if (!map[agent][provider]) {
        errors.push(`Missing model for agent "${agent}" on provider "${provider}"`);
      }
    }
  }
  return errors;
}

export function allProviders() {
  const map = load();
  const set = new Set();
  for (const agent of Object.keys(map)) {
    for (const provider of Object.keys(map[agent])) set.add(provider);
  }
  return [...set];
}
