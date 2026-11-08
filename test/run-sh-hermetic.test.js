import { describe, it, before, after } from 'node:test';
import assert from 'node:assert/strict';
import { chmodSync, copyFileSync, existsSync, mkdirSync, readFileSync, writeFileSync } from 'node:fs';
import { dirname, join } from 'node:path';
import { fileURLToPath } from 'node:url';
import { cleanup, makeTmp, runRunSh } from './helpers.js';

const ROOT = dirname(dirname(fileURLToPath(import.meta.url)));

describe('run.sh test launcher is hermetic', () => {
  let fakeRoot;
  let stubDir;
  let callLog;
  let stubPath;

  before(() => {
    fakeRoot = makeTmp();
    stubDir = makeTmp();
    callLog = join(stubDir, 'npx-calls.log');

    copyFileSync(join(ROOT, 'run.sh'), join(fakeRoot, 'run.sh'));
    mkdirSync(join(fakeRoot, 'tandem'), { recursive: true });
    copyFileSync(join(ROOT, 'tandem', 'min-versions.env'), join(fakeRoot, 'tandem', 'min-versions.env'));
    mkdirSync(join(fakeRoot, '.agent-workspace'), { recursive: true });
    writeFileSync(
      join(fakeRoot, '.agent-workspace', 'manifest.json'),
      JSON.stringify({ version: '0.0.1', files: {} }),
    );

    const npx = join(stubDir, 'npx');
    writeFileSync(
      npx,
      `#!/bin/sh\necho "$PWD :: $*" >> "${callLog}"\ncase "$*" in *--version*) echo "agent-workspace 9.9.9"; exit 0;; esac\nexit 0\n`,
    );
    chmodSync(npx, 0o755);
    stubPath = `${stubDir}:${process.env.PATH}`;
  });

  after(() => {
    cleanup(fakeRoot);
    cleanup(stubDir);
  });

  const scenario = { args: [], env: { TANDEM_RUNTIME: 'invalid_mode_xyz' } };

  function launch(opts = {}) {
    return runRunSh(scenario.args, {
      script: join(fakeRoot, 'run.sh'),
      env: { ...scenario.env, PATH: stubPath },
      ...opts,
    });
  }

  it('control: without the guard, run.sh self-updates through npx when a newer release exists', () => {
    const r = launch({ skipUpdate: false });
    assert.ok(existsSync(callLog), 'control must reach npx, otherwise this test proves nothing');
    const log = readFileSync(callLog, 'utf8');
    assert.match(log, /update --yes/);
    assert.ok(r.cwd, 'cwd reported');
    cleanup(callLog);
  });

  it('by default the launcher never reaches npx and still reports the invalid runtime (exit 4)', () => {
    const r = launch();
    assert.equal(r.code, 4, r.output);
    assert.match(r.output, /Invalid TANDEM_RUNTIME/);
    assert.equal(existsSync(callLog), false, 'npx must not be called');
  });

  it('runs in an isolated temp directory, never inside the repository', () => {
    const r = launch();
    assert.ok(!r.cwd.startsWith(ROOT), `cwd ${r.cwd} must not be inside ${ROOT}`);
    assert.equal(existsSync(r.cwd), false, 'temp cwd is removed afterwards');
  });

  it('does not scaffold workspace files into the repository root', () => {
    const markers = ['AGENTS.md', 'CLAUDE.md', 'WORKFLOW.md', '.agents', 'knowledgebase', 'specs', 'sprints'];
    const before = markers.map((m) => existsSync(join(ROOT, m)));
    launch();
    const after = markers.map((m) => existsSync(join(ROOT, m)));
    assert.deepEqual(after, before);
  });
});
