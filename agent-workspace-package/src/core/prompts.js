import { createInterface } from 'node:readline/promises';
import { VALID_PROVIDERS } from './providers.js';

function rl() {
  return createInterface({ input: process.stdin, output: process.stdout });
}

async function ask(iface, question, defaultVal) {
  const hint = defaultVal !== undefined ? ` [${defaultVal}]` : '';
  const answer = (await iface.question(`${question}${hint}: `)).trim();
  return answer || defaultVal || '';
}

async function askMultiChoice(iface, question, choices, defaultChoices) {
  const list = choices.map((c, i) => `  ${i + 1}. ${c}`).join('\n');
  const hint = `(comma-separated numbers or "all", default: ${defaultChoices.join(',')})`;
  console.log(`\n${question}\n${list}`);
  const answer = (await iface.question(`${hint}: `)).trim().toLowerCase();

  if (!answer || answer === 'all') return [...choices];

  const parts = answer.split(',').map(s => s.trim()).filter(Boolean);
  const selected = [];
  for (const p of parts) {
    const idx = parseInt(p, 10);
    if (!isNaN(idx) && idx >= 1 && idx <= choices.length) {
      selected.push(choices[idx - 1]);
    } else if (choices.includes(p)) {
      selected.push(p);
    }
  }
  return selected.length > 0 ? [...new Set(selected)] : defaultChoices;
}

async function askYesNo(iface, question, defaultYes = false) {
  const hint = defaultYes ? 'Y/n' : 'y/N';
  const answer = (await iface.question(`${question} [${hint}]: `)).trim().toLowerCase();
  if (!answer) return defaultYes;
  return answer === 'y' || answer === 'yes';
}

export async function gatherInitInfo(flags) {
  if (flags.yes) {
    return {
      projectName: flags.name ?? '',
      providers: validateProviders(flags.providers) ?? ['claude'],
      linkCodebases: flags.linkAll ?? false,
      linkAll: flags.linkAll ?? false,
    };
  }

  const iface = rl();
  try {
    const projectName = flags.name ?? await ask(iface, 'Project name', '');
    const providers = flags.providers
      ? validateProviders(flags.providers)
      : await askMultiChoice(iface, 'Select providers to configure:', VALID_PROVIDERS, ['claude']);
    let linkCodebases = false;
    let linkAll = false;
    if (!flags.noLink) {
      linkCodebases = await askYesNo(iface, 'Link sibling codebases?', false);
      if (linkCodebases && flags.linkAll) linkAll = true;
      else if (linkCodebases) linkAll = await askYesNo(iface, 'Link all siblings?', false);
    }
    return { projectName, providers, linkCodebases, linkAll };
  } finally {
    iface.close();
  }
}

export async function gatherUpdateInfo(flags, manifest) {
  if (manifest) {
    return { providers: manifest.providers ?? ['claude'] };
  }
  if (flags.yes) {
    return { providers: validateProviders(flags.providers) ?? ['claude'] };
  }
  const iface = rl();
  try {
    const providers = flags.providers
      ? validateProviders(flags.providers)
      : await askMultiChoice(iface, 'Select providers to update:', VALID_PROVIDERS, ['claude']);
    return { providers };
  } finally {
    iface.close();
  }
}

function validateProviders(list) {
  if (!list) return null;
  const valid = list.filter(p => VALID_PROVIDERS.includes(p));
  return valid.length > 0 ? valid : ['claude'];
}
