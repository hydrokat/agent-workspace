import { describe, it, before, after, beforeEach, afterEach } from 'node:test';
import assert from 'node:assert/strict';
import { chmodSync, cpSync, existsSync, mkdirSync, readFileSync, realpathSync, symlinkSync, writeFileSync } from 'node:fs';
import { spawnSync } from 'node:child_process';
import { dirname, join } from 'node:path';
import { fileURLToPath } from 'node:url';
import { cleanup, makeTmp } from './helpers.js';

const ROOT = dirname(dirname(fileURLToPath(import.meta.url)));
const BASH_PRELUDE = `set +e
eval "$(sed '$d' "$1")"
set +e +u +o pipefail
ROOT_DIR="$FAKE_ROOT"
WEB_MODE="\${HARNESS_WEB_MODE:-1}"
MODE="\${HARNESS_MODE-yes}"
WEB_BIND="\${HARNESS_BIND:-}"
WEB_PORT="\${HARNESS_PORT:-}"
`;

function writeExecutable(path, body) {
  writeFileSync(path, body);
  chmodSync(path, 0o755);
}

const BUN_STUB = (log) => `#!/bin/sh
echo "$(pwd -P) :: $*" >> "${log}"
if [ "$1" = "--version" ]; then
  [ "$BUN_BROKEN" = "1" ] && exit 126
  echo "1.3.3"
  exit 0
fi
if [ "$1" = "install" ]; then
  [ "$BUN_FAIL" = "install" ] && exit 1
  mkdir -p node_modules
fi
if [ "$1" = "run" ] && [ "$2" = "build" ]; then
  [ "$BUN_FAIL" = "build" ] && exit 1
  mkdir -p .next && echo stub > .next/BUILD_ID
fi
exit 0
`;

describe('run.sh --web installs tandem-web when it is not installed yet', () => {
  let sandbox;
  let fakeRoot;
  let webDir;
  let stubDir;
  let home;
  let bunLog;
  let curlLog;
  let npxLog;

  function makeWorkspace({ webDirPresent = true, localCli = true } = {}) {
    if (localCli) {
      cpSync(join(ROOT, 'bin'), join(fakeRoot, 'bin'), { recursive: true });
      cpSync(join(ROOT, 'src'), join(fakeRoot, 'src'), { recursive: true });
      cpSync(join(ROOT, 'package.json'), join(fakeRoot, 'package.json'));
    }
    cpSync(join(ROOT, 'run.sh'), join(fakeRoot, 'run.sh'));
    mkdirSync(join(fakeRoot, 'tandem'), { recursive: true });
    cpSync(join(ROOT, 'tandem', 'min-versions.env'), join(fakeRoot, 'tandem', 'min-versions.env'));
    if (webDirPresent) {
      mkdirSync(webDir, { recursive: true });
      writeFileSync(join(webDir, 'package.json'), '{"name":"tandem-web"}');
      writeFileSync(join(webDir, 'bun.lock'), '');
    }
  }

  function markInstalled() {
    mkdirSync(join(webDir, 'node_modules'), { recursive: true });
  }

  function markBuilt() {
    mkdirSync(join(webDir, '.next'), { recursive: true });
    writeFileSync(join(webDir, '.next', 'BUILD_ID'), 'prebuilt');
  }

  function runWeb({ mode = 'yes', bind = '', port = '', withBun = true, env = {}, webMode = '1', npx = 'present' } = {}) {
    let path = [stubDir, dirname(process.execPath), '/usr/bin', '/bin'].join(':');
    if (npx === 'absent') {
      const minimal = join(sandbox, 'no-npx-path');
      mkdirSync(minimal, { recursive: true });
      if (!existsSync(join(minimal, 'node'))) symlinkSync(process.execPath, join(minimal, 'node'));
      if (withBun) writeExecutable(join(minimal, 'bun'), BUN_STUB(bunLog));
      path = [minimal, '/usr/bin', '/bin'].join(':');
    }
    const harness = join(sandbox, 'harness.sh');
    writeFileSync(harness, `${BASH_PRELUDE}start_web_if_requested\necho "harness-exit=$?"\n`);
    if (withBun) writeExecutable(join(stubDir, 'bun'), BUN_STUB(bunLog));
    const r = spawnSync('/bin/bash', [harness, join(fakeRoot, 'run.sh')], {
      cwd: fakeRoot,
      encoding: 'utf8',
      env: {
        PATH: path,
        HOME: home,
        XDG_STATE_HOME: join(sandbox, 'state'),
        NO_COLOR: '1',
        FAKE_ROOT: fakeRoot,
        HARNESS_MODE: mode,
        HARNESS_BIND: bind,
        HARNESS_PORT: port,
        HARNESS_WEB_MODE: webMode,
        ...env,
      },
    });
    const output = (r.stdout || '') + (r.stderr || '');
    if (/tandem-web started/.test(output)) waitForServerStart();
    return { output, status: r.status };
  }

  function waitForServerStart(timeoutMs = 5000) {
    const gate = new Int32Array(new SharedArrayBuffer(4));
    const deadline = Date.now() + timeoutMs;
    while (Date.now() < deadline) {
      if (existsSync(bunLog) && /run start/.test(readFileSync(bunLog, 'utf8'))) return;
      Atomics.wait(gate, 0, 0, 25);
    }
    assert.fail('tandem-web reported started but the server command never ran (detached child did not log within 5s)');
  }

  function allBunCalls() {
    if (!existsSync(bunLog)) return [];
    return readFileSync(bunLog, 'utf8').trim().split('\n').filter(Boolean).map((l) => {
      const [cwd, args] = l.split(' :: ');
      return { cwd, args };
    });
  }

  function bunCalls() {
    return allBunCalls().filter((c) => c.args !== '--version');
  }

  before(() => {
    const lastLine = readFileSync(join(ROOT, 'run.sh'), 'utf8').trimEnd().split('\n').pop();
    assert.equal(lastLine, 'main "$@"', 'harness assumes run.sh ends with main "$@"; update the harness if that changes');
  });

  beforeEach(() => {
    sandbox = makeTmp();
    fakeRoot = join(sandbox, 'ws');
    webDir = join(fakeRoot, 'tandem-web');
    stubDir = join(sandbox, 'stubs');
    home = join(sandbox, 'home');
    bunLog = join(sandbox, 'bun.log');
    curlLog = join(sandbox, 'curl.log');
    npxLog = join(sandbox, 'npx.log');
    mkdirSync(fakeRoot, { recursive: true });
    mkdirSync(stubDir, { recursive: true });
    mkdirSync(home, { recursive: true });
    fakeRoot = realpathSync(fakeRoot);
    webDir = join(fakeRoot, 'tandem-web');
    writeExecutable(
      join(stubDir, 'npx'),
      [
        '#!/bin/sh',
        `echo "$*" >> "${npxLog}"`,
        'shift 2',
        `exec "${process.execPath}" "${join(ROOT, 'bin', 'agent-workspace.js')}" "$@"`,
        '',
      ].join('\n'),
    );
    writeExecutable(
      join(stubDir, 'curl'),
      [
        '#!/bin/sh',
        `echo "$*" >> "${curlLog}"`,
        "cat <<'INSTALLER'",
        'mkdir -p "$HOME/.bun/bin"',
        'cat > "$HOME/.bun/bin/bun" <<\'BUN\'',
        BUN_STUB(bunLog),
        'BUN',
        'chmod +x "$HOME/.bun/bin/bun"',
        'INSTALLER',
        '',
      ].join('\n'),
    );
  });

  afterEach(() => cleanup(sandbox));

  it('installs dependencies, then builds, then starts the server (in that order, inside tandem-web)', () => {
    makeWorkspace();
    const r = runWeb({ bind: '100.64.0.2', port: '4100' });
    const calls = bunCalls();
    assert.deepEqual(
      calls.map((c) => c.args),
      ['install --frozen-lockfile', 'run build', 'run start -- -H 100.64.0.2 -p 4100'],
      r.output,
    );
    for (const c of calls) assert.equal(c.cwd, webDir, 'bun must run inside the tandem-web directory');
    assert.match(r.output, /Installing tandem-web dependencies/);
    assert.match(r.output, /Building tandem-web/);
    assert.match(r.output, /tandem-web started/);
    assert.equal(existsSync(npxLog), false, 'a local agent-workspace CLI must be used without npx');
  });

  it('prints the access URL with the token so it can be opened or scanned', () => {
    makeWorkspace();
    const r = runWeb({ bind: '100.64.0.2', port: '4100' });
    assert.match(r.output, /http:\/\/100\.64\.0\.2:4100\/\?token=tandem_[0-9a-f]{32}/);
  });

  it('does not reinstall or rebuild when tandem-web is already installed and built', () => {
    makeWorkspace();
    markInstalled();
    markBuilt();
    const r = runWeb({ bind: '100.64.0.2', port: '4100' });
    assert.deepEqual(bunCalls().map((c) => c.args), ['run start -- -H 100.64.0.2 -p 4100'], r.output);
    assert.doesNotMatch(r.output, /Installing tandem-web dependencies/);
    assert.doesNotMatch(r.output, /Building tandem-web/);
  });

  it('only builds when dependencies are present but the build is missing', () => {
    makeWorkspace();
    markInstalled();
    runWeb({ bind: '100.64.0.2', port: '4100' });
    assert.deepEqual(
      bunCalls().map((c) => c.args),
      ['run build', 'run start -- -H 100.64.0.2 -p 4100'],
    );
  });

  it('only installs when the build exists but dependencies are missing', () => {
    makeWorkspace();
    markBuilt();
    runWeb({ bind: '100.64.0.2', port: '4100' });
    assert.deepEqual(
      bunCalls().map((c) => c.args),
      ['install --frozen-lockfile', 'run start -- -H 100.64.0.2 -p 4100'],
    );
  });

  it('is idempotent: a second --web run does not install or build again', () => {
    makeWorkspace();
    runWeb({ bind: '100.64.0.2', port: '4100' });
    writeFileSync(bunLog, '');
    runWeb({ bind: '100.64.0.2', port: '4100' });
    assert.deepEqual(bunCalls().map((c) => c.args), ['run start -- -H 100.64.0.2 -p 4100']);
  });

  it('does not start the server when installing dependencies fails', () => {
    makeWorkspace();
    const r = runWeb({ env: { BUN_FAIL: 'install' } });
    assert.match(r.output, /bun install failed/);
    assert.deepEqual(bunCalls().map((c) => c.args), ['install --frozen-lockfile']);
    assert.doesNotMatch(r.output, /tandem-web started/);
  });

  it('does not start the server when the build fails', () => {
    makeWorkspace();
    const r = runWeb({ env: { BUN_FAIL: 'build' } });
    assert.match(r.output, /bun run build failed/);
    assert.equal(bunCalls().some((c) => c.args.startsWith('run start')), false);
  });

  it('does nothing unless --web was requested', () => {
    makeWorkspace();
    const r = runWeb({ webMode: '0' });
    assert.deepEqual(bunCalls(), []);
    assert.doesNotMatch(r.output, /tandem-web/);
  });

  describe('dependency check before enabling (bun for this host)', () => {
    const stateScript = () => join(realpathSync(sandbox), 'state', 'tandem', 'install-web-deps.sh');

    it('checks that bun actually runs on this host, and generates nothing when it does', () => {
      makeWorkspace();
      const r = runWeb({ bind: '100.64.0.2', port: '4100' });
      assert.ok(allBunCalls().some((c) => c.args === '--version'), 'bun --version must be run');
      assert.equal(existsSync(stateScript()), false, 'no installer script when nothing is missing');
      assert.doesNotMatch(r.output, /Installing bun|not installed for/);
    });

    it('treats a bun binary that does not run on this host as missing', () => {
      makeWorkspace();
      const r = runWeb({ mode: 'no', env: { BUN_BROKEN: '1' } });
      assert.match(r.output, /Not installing dependencies automatically/);
      assert.match(r.output, /bun/);
      assert.equal(bunCalls().some((c) => c.args.startsWith('run start')), false);
    });

    it('when the user declines automatic install: nothing is downloaded or started, but the manual steps and a reviewable script are provided', () => {
      makeWorkspace();
      const r = runWeb({ mode: 'no', withBun: false });
      assert.match(r.output, /Not installing dependencies automatically/);
      assert.match(r.output, /curl -fsSL https:\/\/bun\.com\/install \| bash/);
      assert.match(r.output, /install-web-deps\.sh/);
      assert.equal(existsSync(curlLog), false, 'must not download anything when declined');
      assert.deepEqual(bunCalls(), []);
      assert.match(readFileSync(stateScript(), 'utf8'), /^#!\/usr\/bin\/env bash\n/);
    });

    it('when the user lets tandem install: runs the generated script from bun.com, then installs and starts tandem-web', () => {
      makeWorkspace();
      const r = runWeb({ mode: 'yes', withBun: false, bind: '100.64.0.2', port: '4100' });
      assert.match(readFileSync(curlLog, 'utf8'), /https:\/\/bun\.com\/install/);
      assert.doesNotMatch(readFileSync(curlLog, 'utf8'), /bun\.sh\/install/);
      assert.match(r.output, /Installing bun for /);
      assert.deepEqual(
        bunCalls().map((c) => c.args),
        ['install --frozen-lockfile', 'run build', 'run start -- -H 100.64.0.2 -p 4100'],
        r.output,
      );
      assert.match(readFileSync(stateScript(), 'utf8'), /set -euo pipefail/);
    });

    it('uses the freshly installed bun from ~/.bun/bin even though it is not on PATH', () => {
      makeWorkspace();
      runWeb({ mode: 'yes', withBun: false, bind: '100.64.0.2', port: '4100' });
      assert.ok(existsSync(join(home, '.bun', 'bin', 'bun')));
      assert.ok(bunCalls().length >= 3);
    });

    it('with no terminal and no --yes it neither prompts nor installs (run.sh interactive mode)', () => {
      makeWorkspace();
      const r = runWeb({ mode: '', withBun: false });
      assert.match(r.output, /no terminal to ask on/);
      assert.match(r.output, /--yes/);
      assert.equal(existsSync(curlLog), false);
      assert.deepEqual(bunCalls(), []);
    });
  });

  describe('when the workspace has no tandem-web (what `init` creates)', () => {
    const version = JSON.parse(readFileSync(join(ROOT, 'package.json'), 'utf8')).version;
    const stateWeb = () => join(realpathSync(sandbox), 'state', 'tandem', 'tandem-web');

    it('installs tandem-web into the state directory, then installs, builds and starts it there', () => {
      makeWorkspace({ webDirPresent: false, localCli: false });
      const r = runWeb({ bind: '100.64.0.2', port: '4100' });
      const calls = bunCalls();
      assert.deepEqual(
        calls.map((c) => c.args),
        ['install --frozen-lockfile', 'run build', 'run start -- -H 100.64.0.2 -p 4100'],
        r.output,
      );
      for (const c of calls) assert.equal(c.cwd, stateWeb(), 'bun must run in the installed copy');
      assert.match(r.output, /Installing tandem-web/);
      assert.ok(existsSync(join(stateWeb(), 'package.json')));
      assert.ok(existsSync(join(stateWeb(), 'src', 'middleware.ts')), 'real tandem-web source was copied');
      assert.equal(readFileSync(join(stateWeb(), '.tandem-version'), 'utf8').trim(), version);
      assert.match(r.output, /http:\/\/100\.64\.0\.2:4100\/\?token=tandem_[0-9a-f]{32}/);
    });

    it('reaches the CLI through npx with the configured spec and forwards --bind and --port', () => {
      makeWorkspace({ webDirPresent: false, localCli: false });
      runWeb({ bind: '100.64.0.2', port: '4100' });
      assert.equal(readFileSync(npxLog, 'utf8').trim(), '--yes hydrokat/agent-workspace web --bind 100.64.0.2 --port 4100 --yes');
    });

    it('never writes tandem-web into the workspace itself', () => {
      makeWorkspace({ webDirPresent: false, localCli: false });
      runWeb({ bind: '100.64.0.2', port: '4100' });
      assert.equal(existsSync(join(fakeRoot, 'tandem-web')), false);
    });

    it('does not copy dev artifacts from the package into the installed copy', () => {
      makeWorkspace({ webDirPresent: false, localCli: false });
      runWeb({ bind: '100.64.0.2', port: '4100' });
      assert.equal(existsSync(join(stateWeb(), 'tests')), false);
      assert.equal(existsSync(join(stateWeb(), 'next-env.d.ts')), false);
    });

    it('is idempotent: the second run neither copies, installs nor builds again', () => {
      makeWorkspace({ webDirPresent: false, localCli: false });
      runWeb({ bind: '100.64.0.2', port: '4100' });
      writeFileSync(join(stateWeb(), 'SENTINEL'), 'keep');
      writeFileSync(bunLog, '');
      runWeb({ bind: '100.64.0.2', port: '4100' });
      assert.ok(existsSync(join(stateWeb(), 'SENTINEL')), 'installed copy must be reused');
      assert.deepEqual(bunCalls().map((c) => c.args), ['run start -- -H 100.64.0.2 -p 4100']);
    });

    it('reinstalls and rebuilds after the package version changes', () => {
      makeWorkspace({ webDirPresent: false, localCli: false });
      runWeb({ bind: '100.64.0.2', port: '4100' });
      writeFileSync(join(stateWeb(), 'SENTINEL'), 'old');
      writeFileSync(join(stateWeb(), '.tandem-version'), '0.0.0\n');
      writeFileSync(bunLog, '');
      runWeb({ bind: '100.64.0.2', port: '4100' });
      assert.equal(existsSync(join(stateWeb(), 'SENTINEL')), false, 'stale copy must be replaced');
      assert.deepEqual(
        bunCalls().map((c) => c.args),
        ['install --frozen-lockfile', 'run build', 'run start -- -H 100.64.0.2 -p 4100'],
      );
      assert.equal(readFileSync(join(stateWeb(), '.tandem-version'), 'utf8').trim(), version);
    });

    it('honours a custom TANDEM_UPDATE_SPEC', () => {
      makeWorkspace({ webDirPresent: false, localCli: false });
      runWeb({ env: { TANDEM_UPDATE_SPEC: 'someorg/agent-workspace-fork' } });
      assert.match(readFileSync(npxLog, 'utf8'), /^--yes someorg\/agent-workspace-fork web/);
    });

    it('rejects an invalid TANDEM_UPDATE_SPEC without running anything', () => {
      makeWorkspace({ webDirPresent: false, localCli: false });
      const r = runWeb({ env: { TANDEM_UPDATE_SPEC: 'bad spec; touch /tmp/pwned' } });
      assert.match(r.output, /Invalid TANDEM_UPDATE_SPEC/);
      assert.equal(existsSync(npxLog), false);
      assert.deepEqual(bunCalls(), []);
      assert.match(r.output, /harness-exit=0/);
    });

    it('explains what is missing instead of silently doing nothing when npx is unavailable', () => {
      makeWorkspace({ webDirPresent: false, localCli: false });
      const probe = spawnSync('/bin/sh', ['-c', 'command -v npx'], { env: { PATH: '/usr/bin:/bin' }, encoding: 'utf8' });
      if (probe.status === 0) return;
      const r = runWeb({ npx: 'absent', bind: '100.64.0.2', port: '4100' });
      assert.match(r.output, /npx/i);
      assert.match(r.output, /tandem-web/i);
      assert.deepEqual(bunCalls(), []);
      assert.match(r.output, /harness-exit=0/);
    });
  });
});
