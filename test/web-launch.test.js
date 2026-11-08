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
