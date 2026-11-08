import { accessSync, constants, existsSync, mkdirSync, readFileSync, writeFileSync, chmodSync } from 'node:fs';
import { homedir } from 'node:os';
import { delimiter, join } from 'node:path';
import { spawnSync } from 'node:child_process';
import { createInterface } from 'node:readline';

export const BUN_DOCS_URL = 'https://bun.com/docs/installation';
const BUN_INSTALL_CMD = 'curl -fsSL https://bun.com/install | bash';
const BUN_INSTALL_PS = 'powershell -c "irm bun.sh/install.ps1|iex"';
const PACKAGE_MANAGERS = ['apt', 'dnf', 'yum', 'pacman', 'apk', 'zypper'];
const KNOWN_TOOLS = ['curl', 'unzip', 'brew', 'scoop', ...PACKAGE_MANAGERS];
const SUPPORTED_OS = new Set(['macos', 'linux', 'windows']);
const SUPPORTED_ARCH = new Set(['x64', 'arm64']);

const OS_LABEL = { macos: 'macOS', linux: 'Linux', windows: 'Windows' };

export function platformLabel(platform) {
  const os = OS_LABEL[platform.os] ?? platform.os;
  const libc = platform.libc ? ` (${platform.libc})` : '';
  return `${os} ${platform.arch}${libc}`;
}

export function detectPlatform({ platform = process.platform, arch = process.arch, exists = existsSync } = {}) {
  const os = { darwin: 'macos', win32: 'windows' }[platform] ?? platform;
  let libc = null;
  if (os === 'linux') {
    const loader = arch === 'arm64' ? 'aarch64' : 'x86_64';
    libc = exists(`/lib/ld-musl-${loader}.so.1`) ? 'musl' : 'glibc';
  }
  return { os, arch, libc };
}

export function detectHostProblem({ platform, run = defaultRun, readFile = (p) => readFileSync(p, 'utf8') }) {
  if (platform.os === 'macos') {
    const r = run('sw_vers', ['-productVersion']);
    const version = r.ok ? r.stdout.trim() : '';
    const major = Number.parseInt(version, 10);
    if (Number.isFinite(major) && major < 13) return `macOS ${version} is older than 13.0`;
    return null;
  }
  if (platform.os === 'linux' && platform.arch === 'x64') {
    try {
      if (!readFile('/proc/cpuinfo').includes('sse4_2')) {
        return 'CPU lacks SSE4.2 (bun needs Intel Nehalem or AMD Bulldozer or newer)';
      }
    } catch {
      return null;
    }
  }
  return null;
}

function defaultRun(cmd, args) {
  const r = spawnSync(cmd, args, { encoding: 'utf8', timeout: 10000 });
  return { ok: r.status === 0 && !r.error, stdout: r.stdout ?? '' };
}

export function findExecutable(name, { env = process.env, extraDirs = [], suffixes } = {}) {
  const exts = suffixes ?? (process.platform === 'win32' ? ['.exe', '.cmd', '.bat', ''] : ['']);
  const dirs = [...(env.PATH ?? '').split(delimiter).filter(Boolean), ...extraDirs];
  for (const dir of dirs) {
    for (const ext of exts) {
      const candidate = join(dir, name + ext);
      try {
        accessSync(candidate, constants.X_OK);
        return candidate;
      } catch {
        // keep looking
      }
    }
  }
  return null;
}

function bunInstallDirs(env) {
  const dirs = [];
  if (env.BUN_INSTALL) dirs.push(join(env.BUN_INSTALL, 'bin'));
  dirs.push(join(env.HOME || env.USERPROFILE || homedir(), '.bun', 'bin'));
  return dirs;
}

export function checkWebDeps({
  env = process.env,
  platform = detectPlatform(),
  needTerminal = false,
  run = defaultRun,
  readFile,
} = {}) {
  const bunPath = findExecutable('bun', { env, extraDirs: bunInstallDirs(env) });
  let bun = 'missing';
  let bunVersion = null;
  if (bunPath) {
    const r = run(bunPath, ['--version']);
    bun = r.ok ? 'ok' : 'broken';
    bunVersion = r.ok ? r.stdout.trim() : null;
  }
  const tools = KNOWN_TOOLS.filter((t) => findExecutable(t, { env }));
  const ttyd = needTerminal ? findExecutable('ttyd', { env }) !== null : false;
  const hostProblem = detectHostProblem({ platform, run, ...(readFile ? { readFile } : {}) });
  return {
    facts: { platform, bun, ttyd, needTerminal, hostProblem, tools },
    bunPath: bun === 'ok' ? bunPath : null,
    bunVersion,
  };
}

function pmInstall(platform, tools, pkg) {
  if (platform.os === 'macos') {
    return tools.includes('brew') ? `brew install ${pkg}` : `Install ${pkg} from its official site, then run this again`;
  }
  const templates = {
    apt: `sudo apt install ${pkg}`,
    dnf: `sudo dnf install ${pkg}`,
    yum: `sudo yum install ${pkg}`,
    pacman: `sudo pacman -S ${pkg}`,
    apk: `apk add ${pkg}`,
    zypper: `sudo zypper install ${pkg}`,
  };
  const manager = PACKAGE_MANAGERS.find((m) => tools.includes(m));
  return templates[manager ?? 'apt'];
}

function buildScript(platform, autoInstall) {
  const label = platformLabel(platform);
  const installBun = autoInstall.includes('bun');
  const installTtyd = autoInstall.includes('ttyd');

  if (platform.os === 'windows') {
    const lines = [
      `# tandem-web dependency installer for ${label}`,
      `# Source: ${BUN_DOCS_URL}`,
      "$ErrorActionPreference = 'Stop'",
      '',
      "Write-Host '==> Installing bun'",
      BUN_INSTALL_PS,
      '& "$env:USERPROFILE\\.bun\\bin\\bun" --version',
      '',
      "Write-Host '==> Done'",
      "Write-Host 'If bun is not found in a new terminal, add %USERPROFILE%\\.bun\\bin to your user PATH.'",
      '',
    ];
    return { name: 'install-web-deps.ps1', content: lines.join('\n') };
  }

  const lines = [
    '#!/usr/bin/env bash',
    `# tandem-web dependency installer for ${label}`,
    `# Source: ${BUN_DOCS_URL}`,
    'set -euo pipefail',
    '',
  ];
  if (installBun) {
    lines.push(
      'echo "==> Installing bun"',
      BUN_INSTALL_CMD,
      'export BUN_INSTALL="${BUN_INSTALL:-$HOME/.bun}"',
      'export PATH="$BUN_INSTALL/bin:$PATH"',
      'bun --version',
      '',
    );
  }
  if (installTtyd) {
    lines.push('echo "==> Installing ttyd"', 'brew install ttyd', 'ttyd --version', '');
  }
  lines.push('echo "==> Done"');
  if (installBun) {
    lines.push(
      'echo "If bun is not found in a new shell, add these lines to your shell profile:"',
      "echo '  export BUN_INSTALL=\"$HOME/.bun\"'",
      "echo '  export PATH=\"$BUN_INSTALL/bin:$PATH\"'",
    );
  }
  lines.push('');
  return { name: 'install-web-deps.sh', content: lines.join('\n') };
}

export function planFromFacts(facts) {
  const { platform, bun, ttyd, needTerminal, hostProblem, tools } = facts;
  const bunMissing = bun !== 'ok';
  const bunBroken = bun === 'broken';
  const ttydMissing = Boolean(needTerminal) && !ttyd;

  let unsupportedReason = null;
  if (bunMissing) {
    if (!SUPPORTED_OS.has(platform.os) || !SUPPORTED_ARCH.has(platform.arch)) {
      unsupportedReason = `no official bun build for ${platform.os} ${platform.arch}`;
    } else if (hostProblem) {
      unsupportedReason = hostProblem;
    }
  }
  const supported = unsupportedReason === null;

  const unix = platform.os === 'macos' || platform.os === 'linux';
  const prerequisites = [];
  if (bunMissing && supported && unix) {
    if (!tools.includes('curl')) prerequisites.push('curl');
    if (platform.os === 'linux' && !tools.includes('unzip')) prerequisites.push('unzip');
  }

  const autoInstall = [];
  if (bunMissing && supported && prerequisites.length === 0) autoInstall.push('bun');
  if (ttydMissing && platform.os === 'macos' && tools.includes('brew')) autoInstall.push('ttyd');

  const manual = [];
  for (const dep of prerequisites) {
    manual.push({ dep, commands: [pmInstall(platform, tools, dep)] });
  }
  if (bunMissing) {
    let commands;
    if (!supported) {
      commands = [`See ${BUN_DOCS_URL} (${unsupportedReason})`];
    } else if (platform.os === 'windows') {
      commands = [BUN_INSTALL_PS, ...(tools.includes('scoop') ? ['scoop install bun'] : [])];
    } else {
      commands = [BUN_INSTALL_CMD, ...(tools.includes('brew') ? ['brew install oven-sh/bun/bun'] : [])];
    }
    manual.push({ dep: 'bun', commands });
  }
  if (ttydMissing) {
    let commands;
    if (platform.os === 'macos') {
      commands = tools.includes('brew')
        ? ['brew install ttyd']
        : ['Install Homebrew from https://brew.sh, then run: brew install ttyd'];
    } else if (platform.os === 'linux') {
      commands = [pmInstall(platform, tools, 'ttyd')];
    } else {
      commands = tools.includes('scoop') ? ['scoop install ttyd'] : ['See https://github.com/tsl0922/ttyd'];
    }
    manual.push({ dep: 'ttyd', commands });
  }

  return {
    supported,
    unsupportedReason,
    bunMissing,
    bunBroken,
    ttydMissing,
    canAutoInstall: autoInstall.length > 0,
    autoInstall,
    manual,
    script: autoInstall.length > 0 ? buildScript(platform, autoInstall) : null,
  };
}

function manualLines(plan, scriptPath) {
  const lines = ['Install the missing dependencies yourself, then run this again:'];
  for (const { dep, commands } of plan.manual) {
    lines.push(`  ${dep}:`);
    for (const c of commands) lines.push(`    ${c}`);
  }
  if (scriptPath) {
    const runner = scriptPath.endsWith('.ps1') ? `powershell -File ${scriptPath}` : `bash ${scriptPath}`;
    lines.push(`A script with the same steps is saved at ${scriptPath} (review it, then run: ${runner}).`);
  }
  return lines;
}

function writeScript(stateDir, script) {
  mkdirSync(stateDir, { recursive: true });
  const path = join(stateDir, script.name);
  writeFileSync(path, script.content, { mode: 0o700 });
  chmodSync(path, 0o700);
  return path;
}

function defaultAsk({ question, choices, scriptPath }) {
  const rl = createInterface({ input: process.stdin, output: process.stdout });
  const lines = [question, `  (script: ${scriptPath})`, ...choices.map((c, i) => `  ${i + 1}) ${c.label}`)];
  return new Promise((resolve) => {
    rl.question(`${lines.join('\n')}\nChoose [1-${choices.length}]: `, (answer) => {
      rl.close();
      const picked = choices[Number.parseInt(answer.trim(), 10) - 1];
      resolve(picked ? picked.value : 'manual');
    });
  });
}

function runnerFor(scriptPath) {
  return scriptPath.endsWith('.ps1')
    ? ['powershell', ['-NoProfile', '-ExecutionPolicy', 'Bypass', '-File', scriptPath]]
    : ['bash', [scriptPath]];
}

export async function ensureWebDeps({
  stateDir,
  yes = false,
  noInstall = false,
  isTty = Boolean(process.stdin.isTTY && process.stdout.isTTY),
  needTerminal = false,
  check = () => checkWebDeps({ needTerminal }),
  ask = defaultAsk,
  runScript = (cmd, args) => spawnSync(cmd, args, { stdio: 'inherit' }),
  print = (line) => console.log(line),
} = {}) {
  const report = check();
  const plan = planFromFacts(report.facts);
  const label = platformLabel(report.facts.platform);

  if (!plan.bunMissing) {
    if (plan.ttydMissing) {
      print(`Optional: ttyd is not installed, so terminal attach will be unavailable on ${label}.`);
      for (const m of plan.manual.filter((x) => x.dep === 'ttyd')) {
        for (const c of m.commands) print(`  ${c}`);
      }
    }
    return { ok: true, action: 'present', bunPath: report.bunPath, bunVersion: report.bunVersion };
  }

  if (!plan.supported) {
    print(`bun is required for tandem-web but is not available for ${label}: ${plan.unsupportedReason}.`);
    print(`See ${BUN_DOCS_URL}`);
    return { ok: false, action: 'unsupported' };
  }

  const scriptPath = plan.script ? writeScript(stateDir, plan.script) : null;

  if (!plan.canAutoInstall) {
    print(`bun is required for tandem-web and tandem cannot install it for you on ${label}.`);
    for (const line of manualLines(plan, null)) print(line);
    return { ok: false, action: 'manual' };
  }

  const missing = plan.autoInstall.join(', ');
  let choice;
  if (noInstall) {
    print(`Not installing dependencies automatically. Missing on ${label}: ${missing}.`);
    for (const line of manualLines(plan, scriptPath)) print(line);
    return { ok: false, action: 'manual' };
  }
  if (yes) {
    choice = 'install';
  } else if (!isTty) {
    print(`bun is required for tandem-web but is not installed for ${label}, and there is no terminal to ask on.`);
    print('Re-run with --yes to let tandem install it, or install it yourself:');
    for (const line of manualLines(plan, scriptPath).slice(1)) print(line);
    return { ok: false, action: 'manual' };
  } else {
    choice = await ask({
      question: `tandem-web needs ${missing}, which ${plan.autoInstall.length > 1 ? 'are' : 'is'} not installed for ${label}.`,
      choices: [
        { value: 'install', label: 'Let tandem install it (runs the script shown)' },
        { value: 'manual', label: "I'll install it myself" },
      ],
      scriptPath,
    });
  }

  if (choice !== 'install') {
    for (const line of manualLines(plan, scriptPath)) print(line);
    return { ok: false, action: 'manual' };
  }

  print(`Installing ${missing} for ${label} using ${scriptPath} ...`);
  const [cmd, args] = runnerFor(scriptPath);
  const result = runScript(cmd, args);
  if (result.status !== 0) {
    print(`The installer script failed (exit status ${result.status ?? 'unknown'}). Script: ${scriptPath}`);
    return { ok: false, action: 'failed' };
  }

  const after = check();
  if (planFromFacts(after.facts).bunMissing) {
    print(`The installer finished, but bun is still not available. Could not verify it. Open a new terminal or check that ~/.bun/bin is on your PATH.`);
    return { ok: false, action: 'failed' };
  }
  print(`bun ${after.bunVersion ?? ''} is installed.`.replace('  ', ' '));
  return { ok: true, action: 'installed', bunPath: after.bunPath, bunVersion: after.bunVersion };
}
