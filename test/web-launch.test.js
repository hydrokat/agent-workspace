import { describe, it } from 'node:test';
import assert from 'node:assert/strict';
import {
  generateAuthToken,
  isStrongToken,
  buildServerSpawn,
  buildAccessUrl,
  isLoopback,
  renderQr,
} from '../src/commands/web.js';

describe('web launcher hardening', () => {
  it('generates unguessable, unique tokens in the shared tandem_<32 hex> format', () => {
    const a = generateAuthToken();
    const b = generateAuthToken();
    assert.match(a, /^tandem_[0-9a-f]{32}$/);
    assert.notEqual(a, b);
  });

  it('rejects the weak Math.random-style tokens written by the first p-007 launcher', () => {
    assert.equal(isStrongToken('tandem_k3j2h1g0f9d8s7a6mm1xk2'), false);
    assert.equal(isStrongToken(''), false);
    assert.equal(isStrongToken(undefined), false);
    assert.equal(isStrongToken(generateAuthToken()), true);
  });

  it('passes the auth token and the bind address to the server explicitly', () => {
    const cfg = { bind_ip: '100.64.0.2', port: 4000, auth_token: generateAuthToken() };
    const { args, env } = buildServerSpawn(cfg, { PATH: '/bin' });
    assert.equal(env.TANDEM_AUTH_TOKEN, cfg.auth_token);
    assert.equal(env.HOSTNAME, '100.64.0.2');
    assert.equal(env.PORT, '4000');
    const hi = args.indexOf('-H');
    assert.ok(hi >= 0 && args[hi + 1] === '100.64.0.2', 'must pass -H <ip> because next start ignores HOSTNAME');
    const pi = args.indexOf('-p');
    assert.ok(pi >= 0 && args[pi + 1] === '4000');
  });

  it('refuses to build a spawn for a wildcard bind or missing token', () => {
    assert.throws(() => buildServerSpawn({ bind_ip: '0.0.0.0', port: 3940, auth_token: generateAuthToken() }, {}), /wildcard/);
    assert.throws(() => buildServerSpawn({ bind_ip: '100.64.0.2', port: 3940, auth_token: '' }, {}), /token/);
  });

  it('builds the access URL with the token', () => {
    const cfg = { bind_ip: '100.64.0.2', port: 3940, auth_token: 'tandem_x' };
    assert.equal(buildAccessUrl(cfg), 'http://100.64.0.2:3940/?token=tandem_x');
  });

  it('detects loopback binds that other devices cannot reach', () => {
    assert.equal(isLoopback('127.0.0.1'), true);
    assert.equal(isLoopback('127.9.9.9'), true);
    assert.equal(isLoopback('100.64.0.2'), false);
  });

  it('renders a scannable QR block for a URL', async () => {
    const out = await renderQr('http://100.64.0.2:3940/?token=tandem_x');
    assert.ok(out.split('\n').length > 10);
    assert.match(out, /[▀▄█]/);
  });
});

describe('web launcher bootstrap', () => {
  it('requires install when node_modules is missing and build when BUILD_ID is missing', async () => {
    const { mkdtempSync, mkdirSync, writeFileSync } = await import('node:fs');
    const { tmpdir } = await import('node:os');
    const { join } = await import('node:path');
    const { needsInstall, needsBuild } = await import('../src/commands/web.js');
    const dir = mkdtempSync(join(tmpdir(), 'tw-'));
    assert.equal(needsInstall(dir), true);
    assert.equal(needsBuild(dir), true);
    mkdirSync(join(dir, 'node_modules'));
    assert.equal(needsInstall(dir), false);
    mkdirSync(join(dir, '.next'));
    assert.equal(needsBuild(dir), true);
    writeFileSync(join(dir, '.next', 'BUILD_ID'), 'x');
    assert.equal(needsBuild(dir), false);
  });

  it('runs install before build, only what is needed, and stops on failure', async () => {
    const { bootstrapWeb } = await import('../src/commands/web.js');
    const calls = [];
    const ok = (args) => { calls.push(args.join(' ')); return 0; };
    await bootstrapWeb('/nonexistent-web-dir', ok);
    assert.deepEqual(calls, ['install --frozen-lockfile', 'run build']);

    const failing = (args) => { calls.push(args.join(' ')); return 1; };
    calls.length = 0;
    await assert.rejects(bootstrapWeb('/nonexistent-web-dir', failing), /bun install failed/);
    assert.deepEqual(calls, ['install --frozen-lockfile']);
  });
});

describe('tandem-web source resolution and install', () => {
  async function fixture() {
    const { mkdtempSync, mkdirSync, writeFileSync } = await import('node:fs');
    const { tmpdir } = await import('node:os');
    const { join } = await import('node:path');
    const base = mkdtempSync(join(tmpdir(), 'tw-resolve-'));
    const bundled = join(base, 'pkg', 'tandem-web');
    mkdirSync(join(bundled, 'src', 'app'), { recursive: true });
    mkdirSync(join(bundled, 'node_modules', 'left-pad'), { recursive: true });
    mkdirSync(join(bundled, '.next'), { recursive: true });
    mkdirSync(join(bundled, 'tests'), { recursive: true });
    writeFileSync(join(bundled, 'package.json'), '{"name":"tandem-web"}');
    writeFileSync(join(bundled, 'bun.lock'), 'lock');
    writeFileSync(join(bundled, 'src', 'app', 'page.tsx'), 'export default 1');
    writeFileSync(join(bundled, 'node_modules', 'left-pad', 'index.js'), 'x');
    writeFileSync(join(bundled, '.next', 'BUILD_ID'), 'stale-dev-build');
    writeFileSync(join(bundled, 'tests', 'a.test.ts'), 't');
    writeFileSync(join(bundled, 'next-env.d.ts'), '');
    writeFileSync(join(bundled, 'tsconfig.tsbuildinfo'), '');
    const cwd = join(base, 'ws');
    mkdirSync(cwd, { recursive: true });
    return { base, bundled, cwd, installed: join(base, 'state', 'tandem-web') };
  }

  it('prefers a tandem-web directory inside the workspace (dev checkout)', async () => {
    const { resolveWebDir } = await import('../src/commands/web.js');
    const { mkdirSync, writeFileSync } = await import('node:fs');
    const { join } = await import('node:path');
    const f = await fixture();
    mkdirSync(join(f.cwd, 'tandem-web'));
    writeFileSync(join(f.cwd, 'tandem-web', 'package.json'), '{}');
    const r = resolveWebDir({ cwd: f.cwd, installedDir: f.installed, bundledDir: f.bundled, version: '1.0.0' });
    assert.deepEqual(r, { dir: join(f.cwd, 'tandem-web'), source: 'workspace', installedNow: false });
  });

  it('ignores an empty tandem-web directory in the workspace', async () => {
    const { resolveWebDir } = await import('../src/commands/web.js');
    const { mkdirSync } = await import('node:fs');
    const { join } = await import('node:path');
    const f = await fixture();
    mkdirSync(join(f.cwd, 'tandem-web'));
    const r = resolveWebDir({ cwd: f.cwd, installedDir: f.installed, bundledDir: f.bundled, version: '1.0.0' });
    assert.equal(r.source, 'installed');
  });

  it('installs the bundled source into the state dir on first use and stamps the version', async () => {
    const { resolveWebDir } = await import('../src/commands/web.js');
    const { existsSync, readFileSync } = await import('node:fs');
    const { join } = await import('node:path');
    const f = await fixture();
    const r = resolveWebDir({ cwd: f.cwd, installedDir: f.installed, bundledDir: f.bundled, version: '1.2.3' });
    assert.deepEqual(r, { dir: f.installed, source: 'installed', installedNow: true });
    assert.equal(readFileSync(join(f.installed, '.tandem-version'), 'utf8').trim(), '1.2.3');
    assert.ok(existsSync(join(f.installed, 'package.json')));
    assert.ok(existsSync(join(f.installed, 'bun.lock')));
    assert.ok(existsSync(join(f.installed, 'src', 'app', 'page.tsx')));
  });

  it('does not copy dev artifacts (node_modules, .next, tests, tsbuildinfo, next-env.d.ts)', async () => {
    const { resolveWebDir, needsInstall, needsBuild } = await import('../src/commands/web.js');
    const { existsSync } = await import('node:fs');
    const { join } = await import('node:path');
    const f = await fixture();
    resolveWebDir({ cwd: f.cwd, installedDir: f.installed, bundledDir: f.bundled, version: '1.0.0' });
    for (const p of ['node_modules', '.next', 'tests', 'next-env.d.ts', 'tsconfig.tsbuildinfo']) {
      assert.equal(existsSync(join(f.installed, p)), false, `${p} must not be copied`);
    }
    assert.equal(needsInstall(f.installed), true);
    assert.equal(needsBuild(f.installed), true);
  });

  it('reuses an installed copy of the same version without touching it', async () => {
    const { resolveWebDir } = await import('../src/commands/web.js');
    const { existsSync, writeFileSync } = await import('node:fs');
    const { join } = await import('node:path');
    const f = await fixture();
    resolveWebDir({ cwd: f.cwd, installedDir: f.installed, bundledDir: f.bundled, version: '1.0.0' });
    writeFileSync(join(f.installed, 'SENTINEL'), 'keep me');
    const r = resolveWebDir({ cwd: f.cwd, installedDir: f.installed, bundledDir: f.bundled, version: '1.0.0' });
    assert.deepEqual(r, { dir: f.installed, source: 'installed', installedNow: false });
    assert.ok(existsSync(join(f.installed, 'SENTINEL')));
  });

  it('replaces an installed copy from a different version (fresh install and build)', async () => {
    const { resolveWebDir } = await import('../src/commands/web.js');
    const { existsSync, mkdirSync, readFileSync, writeFileSync } = await import('node:fs');
    const { join } = await import('node:path');
    const f = await fixture();
    resolveWebDir({ cwd: f.cwd, installedDir: f.installed, bundledDir: f.bundled, version: '1.0.0' });
    mkdirSync(join(f.installed, 'node_modules'));
    writeFileSync(join(f.installed, 'SENTINEL'), 'old');
    const r = resolveWebDir({ cwd: f.cwd, installedDir: f.installed, bundledDir: f.bundled, version: '2.0.0' });
    assert.equal(r.installedNow, true);
    assert.equal(existsSync(join(f.installed, 'SENTINEL')), false, 'old copy removed');
    assert.equal(existsSync(join(f.installed, 'node_modules')), false, 'old dependencies removed');
    assert.equal(readFileSync(join(f.installed, '.tandem-version'), 'utf8').trim(), '2.0.0');
  });

  it('treats an installed dir without a version stamp as stale', async () => {
    const { resolveWebDir } = await import('../src/commands/web.js');
    const { mkdirSync, writeFileSync } = await import('node:fs');
    const { join } = await import('node:path');
    const f = await fixture();
    mkdirSync(f.installed, { recursive: true });
    writeFileSync(join(f.installed, 'package.json'), '{"name":"mystery"}');
    const r = resolveWebDir({ cwd: f.cwd, installedDir: f.installed, bundledDir: f.bundled, version: '1.0.0' });
    assert.equal(r.installedNow, true);
  });

  it('fails with a clear error when no source can be found anywhere', async () => {
    const { resolveWebDir } = await import('../src/commands/web.js');
    const { join } = await import('node:path');
    const f = await fixture();
    assert.throws(
      () => resolveWebDir({ cwd: f.cwd, installedDir: f.installed, bundledDir: join(f.base, 'missing'), version: '1.0.0' }),
      /tandem-web source not found/,
    );
  });

  it('exposes the package version used for the stamp', async () => {
    const { packageVersion } = await import('../src/commands/web.js');
    const { readFileSync } = await import('node:fs');
    const pkg = JSON.parse(readFileSync(new URL('../package.json', import.meta.url), 'utf8'));
    assert.equal(packageVersion(), pkg.version);
  });
});
