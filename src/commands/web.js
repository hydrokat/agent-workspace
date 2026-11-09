import { cpSync, existsSync, readFileSync, writeFileSync, unlinkSync, mkdirSync, rmSync, realpathSync } from 'node:fs';
import { homedir } from 'node:os';
import { randomBytes } from 'node:crypto';
import { join, relative, sep, dirname } from 'node:path';
import { fileURLToPath } from 'node:url';
import { checkWebDeps, ensureWebDeps } from './web-deps.js';
import { spawn, spawnSync, execFile } from 'node:child_process';
import { promisify } from 'node:util';

const execFileAsync = promisify(execFile);
const MODULE_DIR = dirname(fileURLToPath(import.meta.url));
export const WEB_VERSION_STAMP = '.tandem-version';
export const WEB_DAEMON_VERSION_FILE = 'tandem-web.version';
const COPY_EXCLUDE_TOP_LEVEL = new Set(['node_modules', '.next', 'tests', 'next-env.d.ts']);

function getStateDir() {
  const xdg = process.env.XDG_STATE_HOME;
  return xdg && xdg.trim().length > 0
    ? join(xdg, 'tandem')
    : join(homedir(), '.local', 'state', 'tandem');
}

function getWebConfigPath() {
  return join(getStateDir(), 'web-config.json');
}

function getWebPidPath() {
  return join(getStateDir(), 'tandem-web.pid');
}

function getWebDaemonVersionPath() {
  return join(getStateDir(), WEB_DAEMON_VERSION_FILE);
}

export function generateAuthToken() {
  return `tandem_${randomBytes(16).toString('hex')}`;
}

export function isStrongToken(token) {
  return typeof token === 'string' && /^tandem_[0-9a-f]{32}$/.test(token);
}

export function isLoopback(ip) {
  return typeof ip === 'string' && ip.startsWith('127.');
}

export function buildAccessUrl(cfg, workspaceSocket) {
  const base = `http://${cfg.bind_ip}:${cfg.port}/?token=${cfg.auth_token}`;
  if (workspaceSocket) {
    return `${base}&workspace=${encodeURIComponent(workspaceSocket)}`;
  }
  return base;
}

export function buildServerSpawn(cfg, baseEnv) {
  if (cfg.bind_ip === '0.0.0.0' || cfg.bind_ip === '::') {
    throw new Error('Refusing to start tandem-web on a wildcard address (use a specific IP).');
  }
  if (!cfg.auth_token) {
    throw new Error('Refusing to start tandem-web without an auth token.');
  }
  const defaultTandemBin = join(
    getStateDir(),
    'bin',
    'current',
    'tandem',
    process.platform === 'win32' ? 'tandem.exe' : 'tandem'
  );
  const tandemBin =
    baseEnv?.TANDEM_BIN ||
    baseEnv?.TANDEM_SELF ||
    (existsSync(defaultTandemBin) ? defaultTandemBin : undefined);

  return {
    args: ['run', 'start', '--', '-H', cfg.bind_ip, '-p', String(cfg.port)],
    env: {
      ...baseEnv,
      PORT: String(cfg.port),
      HOSTNAME: cfg.bind_ip,
      TANDEM_AUTH_TOKEN: cfg.auth_token,
      NODE_ENV: 'production',
      ...(tandemBin ? { TANDEM_BIN: tandemBin } : {}),
    },
  };
}

export function packageVersion() {
  return JSON.parse(readFileSync(join(MODULE_DIR, '..', '..', 'package.json'), 'utf8')).version;
}

export function bundledWebDir() {
  return join(MODULE_DIR, '..', '..', 'tandem-web');
}

export function installedWebDir() {
  const coreDir = join(getStateDir(), 'tandem-web-core-service');
  const legacyDir = join(getStateDir(), 'tandem-web');
  if (existsSync(coreDir)) return coreDir;
  if (existsSync(legacyDir)) return legacyDir;
  return coreDir;
}

export function detectCurrentWorkspaceSocket(cwd = process.cwd(), stateDir = getStateDir()) {
  const p = join(stateDir, 'workspaces.json');
  if (!existsSync(p)) return undefined;
  try {
    const raw = JSON.parse(readFileSync(p, 'utf8'));
    const list = Array.isArray(raw?.workspaces) ? raw.workspaces : [];
    const realCwd = existsSync(cwd) ? realpathSync(cwd) : cwd;
    const match = list.find((w) => {
      if (!w?.path) return false;
      try {
        const realW = existsSync(w.path) ? realpathSync(w.path) : w.path;
        return realW === realCwd || realCwd.startsWith(realW);
      } catch {
        return false;
      }
    });
    return match?.id;
  } catch {
    return undefined;
  }
}

function hasWebSource(dir) {
  return existsSync(join(dir, 'package.json'));
}

function installWebSource(bundledDir, installedDir, version) {
  rmSync(installedDir, { recursive: true, force: true });
  mkdirSync(installedDir, { recursive: true });
  cpSync(bundledDir, installedDir, {
    recursive: true,
    filter: (src) => {
      const rel = relative(bundledDir, src);
      if (rel === '') return true;
      const top = rel.split(sep)[0];
      if (COPY_EXCLUDE_TOP_LEVEL.has(top)) return false;
      return !rel.endsWith('.tsbuildinfo');
    },
  });
  writeFileSync(join(installedDir, WEB_VERSION_STAMP), `${version}\n`);
}

export function refreshInstalledWebOnUpdate({ dryRun = false } = {}) {
  if (dryRun) return false;
  const stateDir = getStateDir();
  const coreDir = join(stateDir, 'tandem-web-core-service');
  const bundledDir = bundledWebDir();
  if (!hasWebSource(bundledDir)) return false;
  if (!existsSync(coreDir) && !existsSync(getWebConfigPath())) return false;

  const version = packageVersion();
  const stampPath = join(coreDir, WEB_VERSION_STAMP);
  const stamp = existsSync(stampPath) ? readFileSync(stampPath, 'utf8').trim() : '';
  const sourceOutdated = !hasWebSource(coreDir) || stamp !== version;
  if (sourceOutdated) {
    installWebSource(bundledDir, coreDir, version);
  }

  const pidFile = getWebPidPath();
  if (!process.env.NODE_TEST_CONTEXT && existsSync(pidFile)) {
    const pid = Number(readFileSync(pidFile, 'utf8').trim());
    const verPath = getWebDaemonVersionPath();
    const daemonVer = existsSync(verPath) ? readFileSync(verPath, 'utf8').trim() : '';
    if (pid && isPidAlive(pid) && (sourceOutdated || daemonVer !== version || needsBuild(coreDir))) {
      const { bunPath } = checkWebDeps();
      if (bunPath) {
        try {
          const run = runBun(coreDir, bunPath);
          if (needsInstall(coreDir)) run(['install', '--frozen-lockfile']);
          if (needsBuild(coreDir)) run(['run', 'build']);
          if (!needsBuild(coreDir)) {
            try { process.kill(pid, 'SIGTERM'); } catch { /* ignore */ }
            const cfg = loadConfig();
            const { args, env } = buildServerSpawn(cfg, process.env);
            const child = spawn(bunPath, args, {
              cwd: coreDir,
              detached: true,
              stdio: 'ignore',
              env,
            });
            child.unref();
            if (child.pid) {
              writeFileSync(pidFile, `${child.pid}\n`, { mode: 0o600 });
              writeFileSync(verPath, `${version}\n`, { mode: 0o600 });
            }
          }
        } catch {
          // Best-effort daemon refresh during workspace update
        }
      }
    }
  }

  return sourceOutdated;
}

export function resolveWebDir({ cwd, installedDir, bundledDir, version }) {
  const local = join(cwd, 'tandem-web');
  if (hasWebSource(local)) {
    return { dir: local, source: 'workspace', installedNow: false };
  }

  const stampPath = join(installedDir, WEB_VERSION_STAMP);
  const stamp = existsSync(stampPath) ? readFileSync(stampPath, 'utf8').trim() : '';
  if (hasWebSource(installedDir) && stamp === version) {
    return { dir: installedDir, source: 'installed', installedNow: false };
  }

  if (!hasWebSource(bundledDir)) {
    throw new Error(`tandem-web source not found (looked in ${local} and ${bundledDir})`);
  }
  installWebSource(bundledDir, installedDir, version);
  return { dir: installedDir, source: 'installed', installedNow: true };
}

export function needsInstall(webDir) {
  return !existsSync(join(webDir, 'node_modules'));
}

export function needsBuild(webDir) {
  return !existsSync(join(webDir, '.next', 'BUILD_ID'));
}

function runBun(webDir, bun = 'bun') {
  return (args) => {
    const r = spawnSync(bun, args, { cwd: webDir, stdio: 'inherit' });
    return r.status ?? 1;
  };
}

export async function bootstrapWeb(webDir, run = runBun(webDir)) {
  if (needsInstall(webDir)) {
    console.log('Installing tandem-web dependencies (first run)...');
    if (run(['install', '--frozen-lockfile']) !== 0) {
      throw new Error('bun install failed in tandem-web');
    }
  }
  if (needsBuild(webDir)) {
    console.log('Building tandem-web (first run)...');
    if (run(['run', 'build']) !== 0) {
      throw new Error('bun run build failed in tandem-web');
    }
  }
}

export async function renderQr(url) {
  const mod = await import('qrcode-terminal');
  const qr = mod.default ?? mod;
  return await new Promise((resolve) => qr.generate(url, { small: true }, resolve));
}

async function printAccess(cfg, flags, headline, workspaceSocket) {
  const url = buildAccessUrl(cfg, workspaceSocket);
  console.log(
    `${headline}\ntandem-web-core-service is installed on the host machine to manage all tandem sessions over a single shared URL and port.\nAccess URL: ${url}`
  );
  if (flags.noQr) return;
  if (isLoopback(cfg.bind_ip)) {
    console.log('Bound to localhost: other devices cannot connect. Re-run with --bind <vpn-or-lan-ip> to get a QR code for your phone.');
    return;
  }
  try {
    console.log(`\nScan with your phone camera to connect:\n${await renderQr(url)}`);
  } catch {
    console.log('(QR code unavailable: qrcode-terminal is not installed. Open the URL above.)');
  }
}

function isValidIpv4(ip) {
  if (typeof ip !== 'string') return false;
  const parts = ip.split('.');
  if (parts.length !== 4) return false;
  return parts.every((p) => {
    if (!/^\d+$/.test(p)) return false;
    const n = Number.parseInt(p, 10);
    return n >= 0 && n <= 255 && (p === '0' || !p.startsWith('0'));
  });
}

function isPidAlive(pid) {
  try {
    process.kill(pid, 0);
    return true;
  } catch {
    return false;
  }
}

function loadConfig() {
  const p = getWebConfigPath();
  if (!existsSync(p)) {
    return {
      enabled: false,
      first_time_acknowledged: false,
      bind_ip: '127.0.0.1',
      bind_source: 'localhost',
      port: 3940,
      auth_token: generateAuthToken(),
      ttyd_enabled: true,
    };
  }
  const cfg = JSON.parse(readFileSync(p, 'utf8'));
  if (!isStrongToken(cfg.auth_token)) {
    cfg.auth_token = generateAuthToken();
  }
  return cfg;
}

function saveConfig(cfg) {
  const dir = getStateDir();
  mkdirSync(dir, { recursive: true });
  const p = getWebConfigPath();
  writeFileSync(p, JSON.stringify(cfg, null, 2) + '\n', { mode: 0o600 });
}

export async function run(context) {
  const { flags } = context;
  const stateDir = getStateDir();
  mkdirSync(stateDir, { recursive: true });

  const pidFile = getWebPidPath();

  if (flags.listIps) {
    try {
      const { stdout } = await execFileAsync('tandem', ['list-ips', '--json']);
      console.log(stdout.trim());
      return;
    } catch {
      console.log(JSON.stringify({ ips: [{ ip: '127.0.0.1', source: 'localhost', interface: 'lo0', preferred: true }], workspaces: [] }));
      return;
    }
  }

  if (flags.status) {
    if (existsSync(pidFile)) {
      const pid = Number(readFileSync(pidFile, 'utf8').trim());
      if (pid && isPidAlive(pid)) {
        const cfg = loadConfig();
        const workspaceSocket = flags.workspace || detectCurrentWorkspaceSocket(context.cwd, stateDir);
        const url = buildAccessUrl(cfg, workspaceSocket);
        if (flags.json) {
          console.log(
            JSON.stringify({
              status: 'running',
              pid,
              url,
              bind_ip: cfg.bind_ip,
              port: cfg.port,
              workspace: workspaceSocket || null,
            })
          );
        } else {
          await printAccess(cfg, flags, `tandem-web is running (pid: ${pid})`, workspaceSocket);
        }
        return;
      }
    }
    if (flags.json) {
      console.log(JSON.stringify({ status: 'stopped' }));
    } else {
      console.log('tandem-web is stopped');
    }
    return;
  }

  if (flags.stop) {
    if (existsSync(pidFile)) {
      const pid = Number(readFileSync(pidFile, 'utf8').trim());
      if (pid && isPidAlive(pid)) {
        try {
          process.kill(pid, 'SIGTERM');
        } catch {
          // Process might already be terminating
        }
      }
      try {
        unlinkSync(pidFile);
      } catch {
        // Ignore unlink failure
      }
      try {
        unlinkSync(getWebDaemonVersionPath());
      } catch {
        // Ignore unlink failure
      }
      const cfg = loadConfig();
      cfg.enabled = false;
      saveConfig(cfg);
      console.log('tandem-web stopped.');
    } else {
      console.log('tandem-web is not running.');
    }
    return;
  }

  // Start web interface
  const cfg = loadConfig();

  if (flags.bind) {
    if (!isValidIpv4(flags.bind)) {
      console.error(`Invalid IPv4 address: '${flags.bind}'`);
      process.exit(1);
    }
    if (flags.bind === '0.0.0.0') {
      console.error("Binding to 0.0.0.0 is prohibited. Choose a specific VPN, LAN, or localhost IP.");
      process.exit(1);
    }
    cfg.bind_ip = flags.bind;
  }

  if (flags.port) {
    const portNum = Number(flags.port);
    if (!portNum || portNum < 1024 || portNum > 65535) {
      console.error(`Port must be between 1024 and 65535. Given: ${flags.port}`);
      process.exit(1);
    }
    cfg.port = portNum;
  }

  const targetVersion = packageVersion();
  const currentInstalledDir = installedWebDir();
  const isRunningDaemonCurrent = () => {
    if (!existsSync(pidFile)) return false;
    const pid = Number(readFileSync(pidFile, 'utf8').trim());
    if (!pid || !isPidAlive(pid)) return false;
    const verFile = getWebDaemonVersionPath();
    if (existsSync(verFile) && readFileSync(verFile, 'utf8').trim() !== targetVersion) return false;
    const stampFile = join(currentInstalledDir, WEB_VERSION_STAMP);
    if (existsSync(stampFile) && readFileSync(stampFile, 'utf8').trim() !== targetVersion) return false;
    if (hasWebSource(currentInstalledDir) && needsBuild(currentInstalledDir)) return false;
    return true;
  };

  const alreadyRunning = isRunningDaemonCurrent();

  let bunPath = 'bun';
  if (!alreadyRunning) {
    const deps = await ensureWebDeps({
      stateDir,
      yes: Boolean(flags.yes),
      noInstall: Boolean(flags.noInstallDeps),
      needTerminal: Boolean(cfg.ttyd_enabled),
    });
    if (!deps.ok) process.exit(1);
    bunPath = deps.bunPath ?? 'bun';
  }

  cfg.enabled = true;
  cfg.first_time_acknowledged = true;
  saveConfig(cfg);

  if (alreadyRunning) {
    const pid = Number(readFileSync(pidFile, 'utf8').trim());
    const workspaceSocket = flags.workspace || detectCurrentWorkspaceSocket(context.cwd, stateDir);
    await printAccess(cfg, flags, `tandem-web is already running (pid: ${pid})`, workspaceSocket);
    return;
  }

  if (existsSync(pidFile)) {
    const stalePid = Number(readFileSync(pidFile, 'utf8').trim());
    if (stalePid && isPidAlive(stalePid)) {
      try {
        process.kill(stalePid, 'SIGTERM');
      } catch {
        // Process already exited
      }
    }
    try {
      unlinkSync(pidFile);
    } catch {
      // Ignore unlink failure
    }
  }

  let webDir;
  try {
    const resolved = resolveWebDir({
      cwd: context.cwd,
      installedDir: currentInstalledDir,
      bundledDir: bundledWebDir(),
      version: targetVersion,
    });
    webDir = resolved.dir;
    if (resolved.installedNow) {
      console.log(`Installed tandem-web into ${webDir}`);
    }
    await bootstrapWeb(webDir, runBun(webDir, bunPath));
  } catch (err) {
    console.error(err.message);
    process.exit(1);
  }

  const { args, env } = buildServerSpawn(cfg, process.env);
  const child = spawn(bunPath, args, {
    cwd: webDir,
    detached: true,
    stdio: 'ignore',
    env,
  });

  child.unref();

  if (child.pid) {
    writeFileSync(pidFile, String(child.pid) + '\n', { mode: 0o600 });
    writeFileSync(getWebDaemonVersionPath(), `${targetVersion}\n`, { mode: 0o600 });
  }

  const workspaceSocket = flags.workspace || detectCurrentWorkspaceSocket(context.cwd, stateDir);
  if (flags.json) {
    console.log(
      JSON.stringify({
        status: 'started',
        pid: child.pid,
        url: buildAccessUrl(cfg, workspaceSocket),
        bind_ip: cfg.bind_ip,
        port: cfg.port,
        workspace: workspaceSocket || null,
      })
    );
  } else {
    await printAccess(cfg, flags, `tandem-web started (pid: ${child.pid})`, workspaceSocket);
  }
}
