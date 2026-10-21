import { describe, it, beforeEach, afterEach } from 'node:test';
import assert from 'node:assert/strict';
import { chmodSync, copyFileSync, mkdirSync, readFileSync, symlinkSync, writeFileSync } from 'node:fs';
import { join, dirname } from 'node:path';
import { fileURLToPath } from 'node:url';
import {
  AI_CLIS, selectClis, parseVersion, versionGe, detect, checkLatest, updateCommand, applyUpdate,
  belowMinimum, loadMinVersions,
} from '../src/core/ai-clis.js';
import { makeTmp, cleanup } from './helpers.js';

const ROOT = dirname(dirname(fileURLToPath(import.meta.url)));
const STUB = join(ROOT, 'test/fixtures/ai-clis/stub.sh');

function fakeDeps({ found = {}, real = {}, versions = {}, replies = {}, writable = true, env = {} } = {}) {
  const calls = [];
  return {
    calls,
    env,
    which: (name) => found[name] ?? null,
    realpath: (p) => real[p] ?? p,
    canWrite: () => writable,
    exec: (argv) => {
      calls.push(argv);
      const key = argv.join(' ');
      if (argv[1] === '--version') return { status: 0, stdout: versions[argv[0]] ?? '', stderr: '' };
      if (key in replies) return { status: 0, stdout: replies[key], stderr: '' };
      return { status: 1, stdout: '', stderr: 'no reply' };
    },
  };
}

describe('selectClis', () => {
  it('always includes claude and agy, plus the configured provider CLIs, in table order', () => {
    assert.deepEqual(selectClis([]), ['claude', 'agy']);
    assert.deepEqual(selectClis(['codex']), ['claude', 'agy', 'codex']);
    assert.deepEqual(selectClis(['antigravity', 'claude']), ['claude', 'agy']);
    assert.deepEqual(selectClis(['codex', 'codex', 'nonsense']), ['claude', 'agy', 'codex']);
  });
});

describe('versions', () => {
  it('parses the first version in a banner and nothing else', () => {
    assert.equal(parseVersion('2.1.305 (Claude Code)'), '2.1.305');
    assert.equal(parseVersion('agy version 1.4.0\nbuild 22'), '1.4.0');
    assert.equal(parseVersion('codex-cli 0.52.0'), '0.52.0');
    assert.equal(parseVersion('tmux 3.3a'), '3.3a');
    assert.equal(parseVersion('1.2.3; rm -rf / $(whoami)'), '1.2.3');
    assert.equal(parseVersion('no digits here'), '');
    assert.equal(parseVersion(undefined), '');
  });

  it('compares like run.sh version_ge', () => {
    const cases = [['2.1.305', '2.1.301', true], ['1.3.6', '1.3.6', true], ['1.3.5', '1.3.6', false],
      ['3.3a', '3.3', true], ['3.3', '3.3a', false], ['2.10', '2.9', true], ['1', '1.0.0', true], ['0.9', '1', false]];
    for (const [a, b, want] of cases) assert.equal(versionGe(a, b), want, `${a} >= ${b}`);
  });
});

describe('detect', () => {
  it('reports a CLI that is not on PATH as not installed', () => {
    assert.deepEqual(detect('claude', fakeDeps()), {
      cli: 'claude', installed: false, path: '', realPath: '', version: '', channel: 'none',
    });
  });

  it('reads the version and classifies the install channel from the real binary path', () => {
    const cases = [
      ['claude', '/home/u/.local/share/claude/versions/2.1.305', 'native'],
      ['claude', '/opt/homebrew/Caskroom/claude-code/2.1.305/claude', 'brew'],
      ['claude', '/usr/lib/node_modules/@anthropic-ai/claude-code/bin/claude.exe', 'npm'],
      ['claude', '/usr/local/bin/claude-weird', 'unknown'],
      ['agy', '/home/u/.local/bin/agy', 'self'],
      ['codex', '/usr/local/lib/node_modules/@openai/codex/bin/codex.js', 'npm'],
      ['codex', '/opt/homebrew/Caskroom/codex/0.52.0/codex', 'brew'],
      ['codex', '/home/u/.local/bin/codex', 'unknown'],
    ];
    for (const [cli, realPath, channel] of cases) {
      const deps = fakeDeps({ found: { [cli]: `/bin/${cli}` }, real: { [`/bin/${cli}`]: realPath },
        versions: { [`/bin/${cli}`]: `${cli} 1.2.3 (build)` } });
      const info = detect(cli, deps);
      assert.equal(info.installed, true);
      assert.equal(info.version, '1.2.3');
      assert.equal(info.channel, channel, `${cli} at ${realPath}`);
      assert.equal(info.realPath, realPath);
    }
  });

  it('survives a failing --version', () => {
    const deps = fakeDeps({ found: { agy: '/bin/agy' } });
    assert.equal(detect('agy', deps).version, '');
  });
});

describe('checkLatest', () => {
  const info = (cli, channel, version = '1.0.0') => ({ cli, installed: true, channel, version, path: '/x', realPath: '/x' });

  it('asks the npm registry for npm installs', () => {
    const deps = fakeDeps({ replies: { 'npm view @openai/codex version': '1.2.0\n' } });
    assert.deepEqual(checkLatest(info('codex', 'npm'), deps), { state: 'available', latest: '1.2.0' });
    const same = fakeDeps({ replies: { 'npm view @openai/codex version': '1.0.0\n' } });
    assert.deepEqual(checkLatest(info('codex', 'npm'), same), { state: 'current', latest: '1.0.0' });
    assert.equal(checkLatest(info('codex', 'npm'), fakeDeps()).state, 'unknown');
  });

  it('asks brew whether the cask is outdated, without letting brew update itself', () => {
    const deps = fakeDeps({ replies: { 'brew outdated --cask --quiet codex': 'codex\n' } });
    assert.equal(checkLatest(info('codex', 'brew'), deps).state, 'available');
    const none = fakeDeps({ replies: { 'brew outdated --cask --quiet codex': '' } });
    assert.equal(checkLatest(info('codex', 'brew'), none).state, 'current');
    assert.equal(checkLatest(info('codex', 'brew'), fakeDeps()).state, 'unknown');
    assert.equal(deps.env.HOMEBREW_NO_AUTO_UPDATE, undefined);
  });

  it('has no check-only source for claude native or agy', () => {
    for (const i of [info('claude', 'native'), info('agy', 'self'), info('claude', 'unknown')]) {
      const deps = fakeDeps();
      assert.deepEqual(checkLatest(i, deps), { state: 'unknown', latest: '' });
      assert.deepEqual(deps.calls, []);
    }
  });

  it('only runs read-only probes', () => {
    const deps = fakeDeps({ replies: { 'npm view @anthropic-ai/claude-code version': '9.0.0', 'brew outdated --cask --quiet claude-code claude-code@latest': '' } });
    checkLatest(info('claude', 'npm'), deps);
    checkLatest(info('claude', 'brew'), deps);
    for (const argv of deps.calls) assert.ok(['npm view', 'brew outdated'].includes(argv.slice(0, 2).join(' ')), argv.join(' '));
  });
});

describe('updateCommand', () => {
  const info = (cli, channel) => ({ cli, installed: true, channel, version: '1.0.0', path: '/x', realPath: '/x' });

  it('uses each CLI\'s own update command', () => {
    assert.deepEqual(updateCommand(info('claude', 'native'), fakeDeps()), { argv: ['claude', 'update'] });
    assert.deepEqual(updateCommand(info('claude', 'brew'), fakeDeps()), { argv: ['claude', 'update'] });
    assert.deepEqual(updateCommand(info('agy', 'self'), fakeDeps()), { argv: ['agy', 'update'] });
  });

  it('updates codex through the channel it was installed with', () => {
    assert.deepEqual(updateCommand(info('codex', 'npm'), fakeDeps()), { argv: ['npm', 'install', '-g', '@openai/codex@latest'] });
    assert.deepEqual(updateCommand(info('codex', 'brew'), fakeDeps()), { argv: ['brew', 'upgrade', '--cask', 'codex'] });
  });

  it('reports a manual command instead of using sudo, curl or a shell', () => {
    const readOnly = updateCommand(info('codex', 'npm'), fakeDeps({ writable: false }));
    assert.match(readOnly.manual, /npm install -g @openai\/codex@latest/);
    const unknown = updateCommand(info('codex', 'unknown'), fakeDeps());
    assert.match(unknown.manual, /chatgpt\.com\/codex\/install\.sh/);
  });

  it('skips claude when DISABLE_UPDATES is set', () => {
    const r = updateCommand(info('claude', 'native'), fakeDeps({ env: { DISABLE_UPDATES: '1' } }));
    assert.match(r.skipped, /DISABLE_UPDATES/);
  });

  it('never produces an argv with sudo, a shell, or a pipe', () => {
    for (const cli of Object.keys(AI_CLIS)) {
      for (const channel of ['native', 'brew', 'npm', 'self', 'unknown']) {
        const r = updateCommand(info(cli, channel), fakeDeps());
        if (r.argv) {
          assert.ok(!r.argv.some((a) => ['sudo', 'sh', 'bash', 'curl'].includes(a) || /[|;&$`]/.test(a)), r.argv.join(' '));
        }
      }
    }
  });
});

describe('applyUpdate (real processes, stubbed CLIs)', () => {
  let dir; let bin; let state; let env;
  beforeEach(() => {
    dir = makeTmp();
    bin = join(dir, 'bin'); state = join(dir, 'state');
    mkdirSync(bin); mkdirSync(state);
    copyFileSync(STUB, join(state, 'stub.sh'));
    chmodSync(join(state, 'stub.sh'), 0o755);
    for (const name of ['claude', 'agy']) symlinkSync(join(state, 'stub.sh'), join(bin, name));
    writeFileSync(join(state, 'claude.version'), 'claude 2.1.305\n');
    writeFileSync(join(state, 'claude.next'), 'claude 2.1.310\n');
    writeFileSync(join(state, 'agy.version'), 'agy 1.3.6\n');
    env = { PATH: `${bin}:/usr/bin:/bin`, STATE: state };
  });
  afterEach(() => cleanup(dir));

  const real = (cli) => detect(cli, { env });
  const calls = () => readFileSync(join(state, 'calls.log'), 'utf8').trim().split('\n');

  it('runs the update, closes stdin, and re-reads the version', () => {
    const before = real('claude');
    assert.equal(before.version, '2.1.305');
    const result = applyUpdate(before, { timeoutMs: 10000 }, { env });
    assert.equal(result.ok, true);
    assert.deepEqual([result.before, result.after, result.changed], ['2.1.305', '2.1.310', true]);
    assert.ok(calls().includes('claude update'));
  });

  it('reports an unchanged version as ok and unchanged', () => {
    const result = applyUpdate(real('agy'), { timeoutMs: 10000 }, { env });
    assert.deepEqual([result.ok, result.changed, result.after], [true, false, '1.3.6']);
  });

  it('reports a failing updater without throwing', () => {
    writeFileSync(join(state, 'agy.update.fail'), '');
    const result = applyUpdate(real('agy'), { timeoutMs: 10000 }, { env });
    assert.equal(result.ok, false);
    assert.match(result.output, /update failed for agy/);
  });

  it('stops an updater that hangs', () => {
    writeFileSync(join(state, 'agy.update.hang'), '');
    const started = Date.now();
    const result = applyUpdate(real('agy'), { timeoutMs: 400 }, { env });
    assert.equal(result.ok, false);
    assert.equal(result.timedOut, true);
    assert.ok(Date.now() - started < 10000);
  });

  it('keeps only the tail of noisy output', () => {
    writeFileSync(join(state, 'agy.update.noisy'), '');
    const result = applyUpdate(real('agy'), { timeoutMs: 10000 }, { env });
    assert.ok(result.output.length <= 4000, `${result.output.length} chars`);
    assert.match(result.output, /noise line from agy updater/);
  });

  it('does not run anything for a manual or skipped command', () => {
    const info = { cli: 'codex', installed: true, channel: 'unknown', version: '0.5.0', path: '/x', realPath: '/x' };
    const result = applyUpdate(info, { timeoutMs: 1000 }, { env });
    assert.equal(result.ok, false);
    assert.match(result.manual, /install\.sh/);
    const disabled = applyUpdate(real('claude'), { timeoutMs: 1000 }, { env: { ...env, DISABLE_UPDATES: '1' } });
    assert.equal(disabled.ok, false);
    assert.match(disabled.skipped, /DISABLE_UPDATES/);
    assert.ok(!readFileSync(join(state, 'calls.log'), 'utf8').includes('update'));
  });
});

describe('minimum versions', () => {
  it('parses min-versions.env as data and compares with version_ge rules', () => {
    const dir = makeTmp();
    try {
      writeFileSync(join(dir, 'min.env'), '# comment\nTANDEM_MIN_CLAUDE=2.1.301\nTANDEM_MIN_AGY=1.3.6\nbad line $(rm -rf /)\nTANDEM_MIN_X=`id`\n');
      const mins = loadMinVersions(join(dir, 'min.env'));
      assert.deepEqual(mins, { TANDEM_MIN_CLAUDE: '2.1.301', TANDEM_MIN_AGY: '1.3.6' });
      assert.equal(belowMinimum({ cli: 'claude', version: '2.1.300' }, mins), true);
      assert.equal(belowMinimum({ cli: 'claude', version: '2.1.301' }, mins), false);
      assert.equal(belowMinimum({ cli: 'codex', version: '0.1.0' }, mins), false);
      assert.equal(belowMinimum({ cli: 'agy', version: '' }, mins), false);
      assert.deepEqual(loadMinVersions(join(dir, 'missing.env')), {});
    } finally { cleanup(dir); }
  });

  it('matches the shipped tandem/min-versions.env', () => {
    const mins = loadMinVersions(join(ROOT, 'tandem/min-versions.env'));
    assert.ok(mins.TANDEM_MIN_CLAUDE && mins.TANDEM_MIN_AGY);
  });
});

describe('run.sh consistency', () => {
  it('run.sh updates claude and agy with the same commands as the table', () => {
    const text = readFileSync(join(ROOT, 'run.sh'), 'utf8');
    const block = text.slice(text.indexOf('update_cmd() {'));
    for (const cli of ['claude', 'agy']) {
      const m = block.match(new RegExp(`${cli}\\) CMD=\\(([^)]*)\\)`));
      assert.ok(m, `${cli} in update_cmd`);
      const channels = AI_CLIS[cli].update;
      assert.deepEqual(m[1].trim().split(/\s+/), channels.default);
    }
  });
});
