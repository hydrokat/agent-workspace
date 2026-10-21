import { spawnSync } from 'node:child_process';
import { accessSync, constants, existsSync, readFileSync, realpathSync, statSync } from 'node:fs';
import { delimiter, dirname, join } from 'node:path';

const OUTPUT_TAIL = 4000;
const DEFAULT_TIMEOUT_MS = 300000;
const PROBE_TIMEOUT_MS = 15000;

const CODEX_SCRIPT = 'curl -fsSL https://chatgpt.com/codex/install.sh | sh';

/**
 * Update commands come from each CLI's own documentation (see knowledgebase/context-history/ai-cli-update-matrix.md).
 * `default` is used for every channel unless the channel has its own entry.
 */
export const AI_CLIS = {
  claude: {
    provider: 'claude',
    npmPackage: '@anthropic-ai/claude-code',
    brewCasks: ['claude-code', 'claude-code@latest'],
    minKey: 'TANDEM_MIN_CLAUDE',
    optOutEnv: 'DISABLE_UPDATES',
    update: { default: ['claude', 'update'] },
  },
  agy: {
    provider: 'antigravity',
    npmPackage: null,
    brewCasks: [],
    minKey: 'TANDEM_MIN_AGY',
    optOutEnv: null,
    update: { default: ['agy', 'update'] },
  },
  codex: {
    provider: 'codex',
    npmPackage: '@openai/codex',
    brewCasks: ['codex'],
    minKey: null,
    optOutEnv: null,
    update: {
      npm: ['npm', 'install', '-g', '@openai/codex@latest'],
      brew: ['brew', 'upgrade', '--cask', 'codex'],
      manual: CODEX_SCRIPT,
    },
  },
};

/** The CLIs to look at: those of the configured providers plus `claude` and `agy`, in table order. */
export function selectClis(providers = []) {
  const wanted = new Set(['claude', 'agy']);
  for (const [name, entry] of Object.entries(AI_CLIS)) {
    if (providers.includes(entry.provider)) wanted.add(name);
  }
  return Object.keys(AI_CLIS).filter((name) => wanted.has(name));
}

/** First dotted number (with an optional trailing letter) in a `--version` banner, like run.sh `extract_version`. */
export function parseVersion(banner) {
  const match = String(banner ?? '').match(/[0-9]+(\.[0-9]+)*[a-z]?/);
  return match ? match[0] : '';
}

/** True when `a >= b`, comparing numerically per component; a letter suffix sorts after none (3.3a > 3.3). */
export function versionGe(a, b) {
  const split = (v) => {
    const suffix = /[a-z]$/.test(v) ? v.slice(-1) : '';
    return { nums: (suffix ? v.slice(0, -1) : v).split('.').map((n) => parseInt(n, 10) || 0), suffix };
  };
  const x = split(a);
  const y = split(b);
  const len = Math.max(x.nums.length, y.nums.length);
  for (let i = 0; i < len; i += 1) {
    const p = x.nums[i] ?? 0;
    const q = y.nums[i] ?? 0;
    if (p !== q) return p > q;
  }
  return x.suffix >= y.suffix;
}

function defaultExec(argv, { timeoutMs = PROBE_TIMEOUT_MS, env } = {}) {
  const r = spawnSync(argv[0], argv.slice(1), {
    encoding: 'utf8', timeout: timeoutMs, env, stdio: ['ignore', 'pipe', 'pipe'], maxBuffer: 16 * 1024 * 1024,
  });
  return {
    status: r.status, stdout: r.stdout ?? '', stderr: r.stderr ?? '',
    timedOut: r.error?.code === 'ETIMEDOUT', error: r.error ? String(r.error.message) : '',
  };
}

function findOnPath(name, env) {
  for (const dir of String(env.PATH ?? '').split(delimiter)) {
    if (!dir) continue;
    const candidate = join(dir, name);
    try {
      accessSync(candidate, constants.X_OK);
      if (statSync(candidate).isFile()) return candidate;
    } catch { /* not here */ }
  }
  return null;
}

function resolveDeps(deps = {}) {
  const env = deps.env ?? process.env;
  return {
    env,
    exec: deps.exec ?? defaultExec,
    which: deps.which ?? ((name) => findOnPath(name, env)),
    realpath: deps.realpath ?? ((p) => { try { return realpathSync(p); } catch { return p; } }),
    canWrite: deps.canWrite ?? ((p) => { try { accessSync(p, constants.W_OK); return true; } catch { return false; } }),
  };
}

function channelOf(cli, realPath) {
  if (realPath.includes('/.local/share/claude/versions/')) return 'native';
  if (realPath.includes('/Caskroom/') || realPath.includes('/Cellar/')) return 'brew';
  const pkg = AI_CLIS[cli].npmPackage;
  if (pkg && realPath.includes(`/node_modules/${pkg}/`)) return 'npm';
  return cli === 'agy' ? 'self' : 'unknown';
}

/** Where `cli` is installed, its version, and how it was installed (native, brew, npm, self, unknown). */
export function detect(cli, deps) {
  const d = resolveDeps(deps);
  const path = d.which(cli);
  if (!path) return { cli, installed: false, path: '', realPath: '', version: '', channel: 'none' };
  const realPath = d.realpath(path);
  const run = d.exec([path, '--version'], { env: d.env });
  return {
    cli, installed: true, path, realPath, version: run.status === 0 ? parseVersion(run.stdout) : '',
    channel: channelOf(cli, realPath),
  };
}

function npmRoot(d) {
  const r = d.exec(['npm', 'root', '-g'], { env: d.env });
  return r.status === 0 ? r.stdout.trim() : '';
}

/**
 * Whether a newer version exists, using only read-only probes: `available`, `current`, or `unknown`.
 * claude's native updater and agy have no check-only mode, so they are always `unknown`.
 */
export function checkLatest(info, deps) {
  const d = resolveDeps(deps);
  const entry = AI_CLIS[info.cli];
  const unknown = { state: 'unknown', latest: '' };
  if (info.channel === 'npm' && entry.npmPackage) {
    const r = d.exec(['npm', 'view', entry.npmPackage, 'version'], { env: d.env });
    const latest = r.status === 0 ? parseVersion(r.stdout) : '';
    if (!latest) return unknown;
    return { state: info.version && versionGe(info.version, latest) ? 'current' : 'available', latest };
  }
  if (info.channel === 'brew' && entry.brewCasks.length) {
    const env = { ...d.env, HOMEBREW_NO_AUTO_UPDATE: '1', HOMEBREW_NO_ENV_HINTS: '1' };
    const r = d.exec(['brew', 'outdated', '--cask', '--quiet', ...entry.brewCasks], { env });
    if (r.status !== 0) return unknown;
    return { state: r.stdout.trim() ? 'available' : 'current', latest: '' };
  }
  return unknown;
}

/**
 * The command that updates `info`: `{ argv }` to run, `{ manual }` to show the user when running it would need
 * sudo or a pipe to a shell, or `{ skipped }` when the CLI's owner has switched updates off.
 */
export function updateCommand(info, deps) {
  const d = resolveDeps(deps);
  const entry = AI_CLIS[info.cli];
  if (entry.optOutEnv && d.env[entry.optOutEnv] === '1') {
    return { skipped: `${entry.optOutEnv}=1 is set, which blocks ${info.cli} updates` };
  }
  const argv = entry.update[info.channel] ?? entry.update.default;
  if (!argv) return { manual: entry.update.manual };
  if (info.channel === 'npm' && argv[0] === 'npm') {
    const root = npmRoot(d);
    if (root && !d.canWrite(root)) {
      return { manual: `${argv.join(' ')}   (npm's global folder is not writable; fix its permissions, do not use sudo)` };
    }
    if (!root && !d.canWrite(dirname(info.realPath))) return { manual: argv.join(' ') };
  }
  return { argv };
}

function tail(text) {
  const cleaned = text.replace(/\x1b\[[0-9;]*[A-Za-z]/g, '').trim();
  return cleaned.length > OUTPUT_TAIL ? cleaned.slice(-OUTPUT_TAIL) : cleaned;
}

/** Runs the update (no shell, stdin closed, bounded time) and re-reads the version. */
export function applyUpdate(info, { timeoutMs = DEFAULT_TIMEOUT_MS } = {}, deps) {
  const d = resolveDeps(deps);
  const plan = updateCommand(info, deps);
  if (!plan.argv) return { ok: false, before: info.version, after: info.version, changed: false, output: '', ...plan };
  const argv = plan.argv[0] === info.cli && info.path ? [info.path, ...plan.argv.slice(1)] : plan.argv;
  const run = d.exec(argv, { timeoutMs, env: d.env });
  const after = detect(info.cli, deps).version || info.version;
  const output = tail(`${run.stdout}\n${run.stderr}`);
  const ok = run.status === 0 && !run.timedOut;
  return {
    ok, before: info.version, after, changed: after !== info.version, output, exitCode: run.status,
    ...(run.timedOut ? { timedOut: true } : {}),
    ...(!ok ? { command: plan.argv.join(' ') } : {}),
  };
}

/** Reads `tandem/min-versions.env` as data: only `KEY=value` lines of plain characters count. */
export function loadMinVersions(file) {
  if (!existsSync(file)) return {};
  const mins = {};
  for (const line of readFileSync(file, 'utf8').split('\n')) {
    const m = line.match(/^([A-Z_]+)=([0-9A-Za-z.-]+)$/);
    if (m) mins[m[1]] = m[2];
  }
  return mins;
}

/** True when the installed version is known and lower than the tandem minimum for that CLI. */
export function belowMinimum(info, mins) {
  const key = AI_CLIS[info.cli]?.minKey;
  const min = key ? mins[key] : '';
  return Boolean(info.version && min && !versionGe(info.version, min));
}
