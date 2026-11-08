import { existsSync, readFileSync, writeFileSync, unlinkSync, mkdirSync } from 'node:fs';
import { homedir } from 'node:os';
import { join } from 'node:path';
import { spawn, execFile } from 'node:child_process';
import { promisify } from 'node:util';

const execFileAsync = promisify(execFile);

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
      auth_token: `tandem_${Math.random().toString(36).slice(2)}${Date.now().toString(36)}`,
      ttyd_enabled: false,
    };
  }
  return JSON.parse(readFileSync(p, 'utf8'));
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
        const url = `http://${cfg.bind_ip}:${cfg.port}/?token=${cfg.auth_token}`;
        if (flags.json) {
          console.log(JSON.stringify({ status: 'running', pid, url, bind_ip: cfg.bind_ip, port: cfg.port }));
        } else {
          console.log(`tandem-web is running (pid: ${pid})\nURL: ${url}`);
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

  cfg.enabled = true;
  cfg.first_time_acknowledged = true;
  saveConfig(cfg);

  if (existsSync(pidFile)) {
    const pid = Number(readFileSync(pidFile, 'utf8').trim());
    if (pid && isPidAlive(pid)) {
      const url = `http://${cfg.bind_ip}:${cfg.port}/?token=${cfg.auth_token}`;
      console.log(`tandem-web is already running (pid: ${pid})\nURL: ${url}`);
      return;
    }
  }

  const tandemWebDir = join(context.cwd, 'tandem-web');
  const webDir = existsSync(tandemWebDir) ? tandemWebDir : join(import.meta.dirname, '..', '..', 'tandem-web');

  const child = spawn('bun', ['run', 'start'], {
    cwd: webDir,
    detached: true,
    stdio: 'ignore',
    env: {
      ...process.env,
      PORT: String(cfg.port),
      HOST: cfg.bind_ip,
    },
  });

  child.unref();

  if (child.pid) {
    writeFileSync(pidFile, String(child.pid) + '\n', { mode: 0o600 });
  }

  const url = `http://${cfg.bind_ip}:${cfg.port}/?token=${cfg.auth_token}`;
  if (flags.json) {
    console.log(JSON.stringify({ status: 'started', pid: child.pid, url, bind_ip: cfg.bind_ip, port: cfg.port }));
  } else {
    console.log(`tandem-web started (pid: ${child.pid})\nAccess URL: ${url}`);
  }
}
