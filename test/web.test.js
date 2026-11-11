import { describe, it, before, after } from 'node:test';
import assert from 'node:assert/strict';
import { existsSync, readFileSync, writeFileSync, mkdirSync } from 'node:fs';
import { join, dirname } from 'node:path';
import { fileURLToPath } from 'node:url';
import { cleanup, makeTmp, runCli as runCliIn } from './helpers.js';
import {
  buildAccessUrl,
  detectCurrentWorkspaceSocket,
  packageVersion,
  refreshInstalledWebOnUpdate,
  webSourceHash,
  needsBuild,
  WEB_SOURCE_HASH_STAMP,
} from '../src/commands/web.js';

const MODULE_DIR = dirname(fileURLToPath(import.meta.url));
const FIXTURE_DIR = join(MODULE_DIR, 'fixtures', 'web-source-hash');
const EXPECTED_SHA256_PATH = join(MODULE_DIR, 'fixtures', 'web-source-hash.EXPECTED_SHA256');

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

  // ----- Part A: hash algorithm cross-check -----

  it('webSourceHash matches the shared fixture digest', async () => {
    assert.ok(existsSync(FIXTURE_DIR), `fixture dir must exist: ${FIXTURE_DIR}`);
    assert.ok(existsSync(EXPECTED_SHA256_PATH), `EXPECTED_SHA256 file must exist`);
    const expected = readFileSync(EXPECTED_SHA256_PATH, 'utf8').trim();
    const got = await webSourceHash(FIXTURE_DIR);
    assert.equal(got, expected, 'Node hash does not match EXPECTED_SHA256');
  });

  it('webSourceHash excludes node_modules, .next, tests, next-env.d.ts, .tsbuildinfo, .tandem-* files', async () => {
    const tmp = makeTmp();
    try {
      mkdirSync(join(tmp, 'src', 'app'), { recursive: true });
      writeFileSync(join(tmp, 'src', 'app', 'page.tsx'), 'included');

      const hashBefore = await webSourceHash(tmp);

      // Add excluded files
      mkdirSync(join(tmp, 'node_modules', 'x'), { recursive: true });
      writeFileSync(join(tmp, 'node_modules', 'x', 'index.js'), 'npm');
      mkdirSync(join(tmp, '.next'), { recursive: true });
      writeFileSync(join(tmp, '.next', 'BUILD_ID'), 'b');
      mkdirSync(join(tmp, 'tests'), { recursive: true });
      writeFileSync(join(tmp, 'tests', 'x.test.ts'), 'test');
      writeFileSync(join(tmp, 'foo.tsbuildinfo'), '{}');
      writeFileSync(join(tmp, '.tandem-version'), '1.0.0');
      writeFileSync(join(tmp, 'next-env.d.ts'), '// next');

      const hashAfter = await webSourceHash(tmp);
      assert.equal(hashBefore, hashAfter, 'excluded files must not change the hash');

      // Changing an included file must change the hash
      writeFileSync(join(tmp, 'src', 'app', 'page.tsx'), 'changed');
      const hashChanged = await webSourceHash(tmp);
      assert.notEqual(hashBefore, hashChanged, 'content change must change the hash');
    } finally {
      cleanup(tmp);
    }
  });

  // ----- Part B: stamp and compare -----

  it('refreshInstalledWebOnUpdate re-installs when the source hash differs at the same version', () => {
    const tmp = makeTmp();
    const prevXdg = process.env.XDG_STATE_HOME;
    try {
      process.env.XDG_STATE_HOME = tmp;
      const version = packageVersion();
      const coreDir = join(tmp, 'tandem', 'tandem-web-core-service');
      mkdirSync(join(coreDir, 'src'), { recursive: true });
      writeFileSync(join(coreDir, 'package.json'), '{}');
      writeFileSync(join(coreDir, 'src', 'old.ts'), 'old-content');
      writeFileSync(join(coreDir, '.tandem-version'), `${version}\n`);
      // Write a source hash stamp that does NOT match the bundled hash
      writeFileSync(join(coreDir, WEB_SOURCE_HASH_STAMP), 'stale-hash\n');
      // No web-config.json → refreshInstalledWebOnUpdate will bail unless coreDir exists
      writeFileSync(join(tmp, 'tandem', 'web-config.json'), '{"enabled":false}');

      const updated = refreshInstalledWebOnUpdate({ dryRun: false });
      assert.equal(updated, true, 'must re-install when source hash differs');

      // Source hash stamp must now be current
      const newStamp = readFileSync(join(coreDir, WEB_SOURCE_HASH_STAMP), 'utf8').trim();
      assert.ok(newStamp.length === 64, `stamp must be a hex sha256, got: ${newStamp}`);
      assert.notEqual(newStamp, 'stale-hash', 'stale stamp must be replaced');
    } finally {
      if (prevXdg === undefined) delete process.env.XDG_STATE_HOME;
      else process.env.XDG_STATE_HOME = prevXdg;
      cleanup(tmp);
    }
  });

  it('needsBuild is true when the build hash differs from the source hash', () => {
    const tmp = makeTmp();
    try {
      mkdirSync(join(tmp, '.next'), { recursive: true });
      writeFileSync(join(tmp, '.next', 'BUILD_ID'), 'b');

      // No stamps at all → needs build
      assert.equal(needsBuild(tmp), true);

      // Source stamp exists, build hash does not match
      writeFileSync(join(tmp, WEB_SOURCE_HASH_STAMP), 'abc123\n');
      writeFileSync(join(tmp, '.next', 'TANDEM_SOURCE_HASH'), 'different\n');
      assert.equal(needsBuild(tmp), true, 'mismatched hashes must need build');

      // Matching hashes → does NOT need build
      writeFileSync(join(tmp, '.next', 'TANDEM_SOURCE_HASH'), 'abc123\n');
      assert.equal(needsBuild(tmp), false, 'matching hashes must not need build');
    } finally {
      cleanup(tmp);
    }
  });
});
