import { describe, it, before, after } from 'node:test';
import assert from 'node:assert/strict';
import { cleanup, makeTmp, runCli as runCliIn } from './helpers.js';

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
});
