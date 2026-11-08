import { describe, it, before, after } from 'node:test';
import assert from 'node:assert/strict';
import { mkdirSync, readFileSync, writeFileSync } from 'node:fs';
import { spawn, spawnSync } from 'node:child_process';
import { dirname, join } from 'node:path';
import { fileURLToPath } from 'node:url';
import { cleanup, makeTmp } from './helpers.js';

const ROOT = dirname(dirname(fileURLToPath(import.meta.url)));

function alive(pid) {
  try {
    process.kill(pid, 0);
    return true;
  } catch {
    return false;
  }
}

describe('the web CLI tests never touch a real tandem-web', () => {
  let ambient;
  let stateDir;
  let fakeServer;
  let configPath;
  let configBefore;

  before(() => {
    ambient = makeTmp();
    stateDir = join(ambient, 'tandem');
    mkdirSync(stateDir, { recursive: true });
    fakeServer = spawn('sleep', ['120'], { stdio: 'ignore', detached: true });
    fakeServer.unref();
    writeFileSync(join(stateDir, 'tandem-web.pid'), `${fakeServer.pid}\n`);
    configPath = join(stateDir, 'web-config.json');
    writeFileSync(
      configPath,
      JSON.stringify({ enabled: true, first_time_acknowledged: true, bind_ip: '100.64.0.2', port: 3940, auth_token: `tandem_${'a'.repeat(32)}`, ttyd_enabled: false }),
    );
    configBefore = readFileSync(configPath, 'utf8');
  });

  after(() => {
    try {
      process.kill(fakeServer.pid, 'SIGKILL');
    } catch {
      // already gone
    }
    cleanup(ambient);
  });

  it('passes while a real server appears to be running in the ambient state dir', () => {
    const env = { ...process.env, XDG_STATE_HOME: ambient, HOME: ambient };
    delete env.NODE_TEST_CONTEXT;
    const r = spawnSync(process.execPath, ['--test', join(ROOT, 'test', 'web.test.js')], {
      encoding: 'utf8',
      env,
    });
    assert.equal(r.status, 0, `${r.stdout}\n${r.stderr}`);
  });

  it('leaves the ambient server running', () => {
    assert.ok(alive(fakeServer.pid), 'the web tests must not stop a server they did not start');
  });

  it('leaves the ambient config and pid file untouched', () => {
    assert.equal(readFileSync(configPath, 'utf8'), configBefore);
    assert.equal(readFileSync(join(stateDir, 'tandem-web.pid'), 'utf8').trim(), String(fakeServer.pid));
  });
});
