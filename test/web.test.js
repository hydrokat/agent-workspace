import { describe, it, before, after } from 'node:test';
import assert from 'node:assert/strict';
import { existsSync, readFileSync, writeFileSync, mkdirSync } from 'node:fs';
import { join } from 'node:path';
import { cleanup, makeTmp, runCli as runCliIn } from './helpers.js';
import {
  buildAccessUrl,
  detectCurrentWorkspaceSocket,
  packageVersion,
  refreshInstalledWebOnUpdate,
} from '../src/commands/web.js';

describe('agent-workspace web command', () => {
  let stateRoot;
  let isolatedEnv;

  before(() => {
    stateRoot = makeTmp();
    isolatedEnv = { ...process.env, XDG_STATE_HOME: stateRoot, HOME: stateRoot };
  });

  after(() => cleanup(stateRoot));

  const runCli = (args, cwd) => runCliIn(args, cwd, isolatedEnv);

  it('web --status reports stopped when daemon is not running', async () => {
    const { code, stdout } = await runCli(['web', '--status'], process.cwd());
    assert.equal(code, 0);
    assert.ok(stdout.includes('tandem-web is stopped'));
  });

  it('web --status --json reports machine-readable JSON status', async () => {
    const { code, stdout } = await runCli(['web', '--status', '--json'], process.cwd());
    assert.equal(code, 0);
    const data = JSON.parse(stdout);
    assert.equal(data.status, 'stopped');
  });

  it('web --list-ips returns available IP candidates', async () => {
    const { code, stdout } = await runCli(['web', '--list-ips'], process.cwd());
    assert.equal(code, 0);
    const data = JSON.parse(stdout);
    assert.ok(Array.isArray(data.ips), 'data.ips is an array');
  });

  it('web rejects 0.0.0.0 wildcard binding', async () => {
    const { code, stderr } = await runCli(['web', '--bind', '0.0.0.0'], process.cwd());
    assert.notEqual(code, 0);
    assert.ok(stderr.includes('Binding to 0.0.0.0 is prohibited'));
  });

  it('web rejects invalid IPv4 format', async () => {
    const { code, stderr } = await runCli(['web', '--bind', '999.999.999.999'], process.cwd());
    assert.notEqual(code, 0);
    assert.ok(stderr.includes('Invalid IPv4 address'));
  });

  it('web rejects out-of-range port numbers', async () => {
    const { code, stderr } = await runCli(['web', '--port', '80'], process.cwd());
    assert.notEqual(code, 0);
    assert.ok(stderr.includes('Port must be between 1024 and 65535'));
  });

  it('web --stop handles non-running gracefully', async () => {
    const { code, stdout } = await runCli(['web', '--stop'], process.cwd());
    assert.equal(code, 0);
    assert.ok(stdout.includes('tandem-web is not running') || stdout.includes('tandem-web stopped'));
  });

  it('buildAccessUrl formats URLs with or without workspace socket', () => {
    const cfg = { bind_ip: '192.168.1.100', port: 3940, auth_token: 'tandem_test123' };
    assert.equal(buildAccessUrl(cfg), 'http://192.168.1.100:3940/?token=tandem_test123');
    assert.equal(
      buildAccessUrl(cfg, 'tandem-85981edb6c'),
      'http://192.168.1.100:3940/?token=tandem_test123&workspace=tandem-85981edb6c'
    );
  });

  it('detectCurrentWorkspaceSocket matches workspace from workspaces.json', () => {
    const tmp = makeTmp();
    const wsDir = join(tmp, 'my-workspace');
    mkdirSync(wsDir, { recursive: true });
    const stateDir = join(tmp, 'state');
    mkdirSync(stateDir, { recursive: true });
    writeFileSync(
      join(stateDir, 'workspaces.json'),
      JSON.stringify({
        workspaces: [{ id: 'tandem-9999999999', path: wsDir, name: 'my-workspace' }],
      })
    );

    const socket = detectCurrentWorkspaceSocket(wsDir, stateDir);
    assert.equal(socket, 'tandem-9999999999');
    cleanup(tmp);
  });

  it('refreshInstalledWebOnUpdate upgrades stale tandem-web-core-service source and clears .next', () => {
    const tmp = makeTmp();
    const prevXdg = process.env.XDG_STATE_HOME;
    try {
      process.env.XDG_STATE_HOME = tmp;
      const coreDir = join(tmp, 'tandem', 'tandem-web-core-service');
      mkdirSync(join(coreDir, '.next'), { recursive: true });
      writeFileSync(join(coreDir, 'package.json'), '{}');
      writeFileSync(join(coreDir, 'server.ts'), 'old-http-server');
      writeFileSync(join(coreDir, '.tandem-version'), '0.0.1\n');
      writeFileSync(join(coreDir, '.next', 'BUILD_ID'), 'old-build');

      const updated = refreshInstalledWebOnUpdate({ dryRun: false });
      assert.equal(updated, true);
      assert.equal(readFileSync(join(coreDir, '.tandem-version'), 'utf8').trim(), packageVersion());
      assert.ok(readFileSync(join(coreDir, 'server.ts'), 'utf8').includes('Bun.serve'));
      assert.equal(existsSync(join(coreDir, '.next', 'BUILD_ID')), false);
    } finally {
      if (prevXdg === undefined) delete process.env.XDG_STATE_HOME;
      else process.env.XDG_STATE_HOME = prevXdg;
      cleanup(tmp);
    }
  });
});
