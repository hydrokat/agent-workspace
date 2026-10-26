import { describe, it, afterEach } from 'node:test';
import assert from 'node:assert/strict';
import { spawn, execFileSync } from 'node:child_process';
import { chmodSync, copyFileSync, mkdirSync, readFileSync, existsSync, symlinkSync, writeFileSync } from 'node:fs';
import { join, dirname } from 'node:path';
import { fileURLToPath } from 'node:url';
import { makeTmp, cleanup } from './helpers.js';

const ROOT = dirname(dirname(fileURLToPath(import.meta.url)));
const CORE = ['sed', 'grep', 'cat', 'tr', 'mktemp', 'sleep', 'kill', 'cut', 'dirname', 'basename', 'rm', 'ln', 'sh', 'head', 'date'];
const MIN = [
  '# test minimums', 'TANDEM_MIN_PYTHON=3.11', 'TANDEM_MIN_GIT=2.30', 'TANDEM_MIN_TMUX=3.3',
  'TANDEM_MIN_UV=0.5', 'TANDEM_MIN_CLAUDE=2.1.301', 'TANDEM_MIN_AGY=1.3.6', '',
].join('\n');

const TOOL_SH = `#!/bin/sh
name=$(basename "$0")
echo "$name $*" >> "$STATE/calls.log"
bump() { echo "$1 9.9.9" > "$STATE/$1.version"; }
install_pkg() { [ -f "$STATE/$1.installfail" ] && { echo "install failed for $1" >&2; exit 1; }; ln -sf "$STATE/tool.sh" "$BIN/$1"; bump "$1"; }
case "$name" in
  brew)
    case "$1" in
      install) install_pkg "$2"; exit 0;;
      upgrade) bump "$2"; exit 0;;
      outdated) [ -f "$STATE/outdated" ] && cat "$STATE/outdated"; exit 0;;
    esac;;
  sudo) exec "$@";;
  apt-get|dnf) if [ "$1" = install ] || [ "$2" = install ]; then for last; do :; done; install_pkg "$last"; fi; exit 0;;
  curl)
    for last; do :; done
    case "$last" in
      *claude.ai*) n=claude;; *antigravity*) n=agy;; *astral*) n=uv;; *) exit 22;;
    esac
    [ -f "$STATE/$n.installfail" ] && exit 22
    echo "ln -sf \\"$STATE/tool.sh\\" \\"$BIN/$n\\"; echo \\"$n 9.9.9\\" > \\"$STATE/$n.version\\""
    exit 0;;
  uv)
    case "$*" in
      "self update --dry-run") [ -f "$STATE/uv.dryrun.fail" ] && { echo "network down" >&2; exit 1; }; [ -f "$STATE/uv.dryrun" ] && cat "$STATE/uv.dryrun"; exit 0;;
      "self update") [ -f "$STATE/uv.noupdate" ] || bump uv; exit 0;;
      sync*) echo "sync-python=$UV_PYTHON" >> "$STATE/calls.log"; exit 0;;
      run*) echo "TANDEM-RAN cwd=$PWD args=$* venv=$UV_PROJECT_ENVIRONMENT python=$UV_PYTHON"; exit 0;;
    esac;;
  claude|agy)
    if [ "$1" = update ]; then [ -f "$STATE/$name.noupdate" ] || bump "$name"; exit 0; fi;;
  npx)
    [ "$1" = "--yes" ] && shift
    shift
    case "$1" in
      --version)
        [ -f "$STATE/npx.offline" ] && exit 1
        if [ -f "$STATE/npx.version" ]; then cat "$STATE/npx.version"; else echo "1.3.0"; fi
        exit 0;;
      update)
        [ -f "$STATE/npx.updatefail" ] && { echo "npx update failed" >&2; exit 1; }
        exit 0;;
    esac
    exit 0;;
esac
case "$1" in --version|-V) cat "$STATE/$name.version"; exit 0;; esac
exit 0
`;

const ok = (v) => ({ git: 'git version 2.54.0 (Apple Git-157)', tmux: 'tmux 3.3a', uv: 'uv 0.10.4 (079e3fd05 2026-02-17)',
  claude: '2.1.301 (Claude Code)', agy: '1.3.6', ...v });

let dirs = [];
afterEach(() => { dirs.forEach(cleanup); dirs = []; });

function setup({ versions = ok(), os = 'Darwin', managers = ['brew'], min = MIN } = {}) {
  const base = makeTmp();
  dirs.push(base);
  const pkg = join(base, 'pkg'), state = join(base, 'state'), bin = join(base, 'bin'), work = join(base, 'work');
  for (const d of [join(pkg, 'tandem'), state, bin, work]) mkdirSync(d, { recursive: true });
  copyFileSync(join(ROOT, 'run.sh'), join(pkg, 'run.sh'));
  writeFileSync(join(pkg, 'tandem/min-versions.env'), min);
  writeFileSync(join(state, 'tool.sh'), TOOL_SH);
  chmodSync(join(state, 'tool.sh'), 0o755);
  for (const c of CORE) symlinkSync(execFileSync('/usr/bin/which', [c]).toString().trim(), join(bin, c));
  symlinkSync(execFileSync('/usr/bin/which', ['bash']).toString().trim(), join(bin, 'bash'));
  writeFileSync(join(bin, 'uname'), `#!/bin/sh\necho ${os}\n`);
  chmodSync(join(bin, 'uname'), 0o755);
  const link = (n) => symlinkSync(join(state, 'tool.sh'), join(bin, n));
  for (const [n, banner] of Object.entries(versions)) {
    if (banner === null) continue;
    link(n);
    writeFileSync(join(state, `${n}.version`), banner + '\n');
  }
  for (const m of [...managers, 'curl']) link(m);
  if (os === 'Linux') link('sudo');
  return { base, pkg, state, bin, work, mark: (f, c = '') => writeFileSync(join(state, f), c) };
}

function run(env, args = [], { stdin = null, extraEnv = {} } = {}) {
  return new Promise((resolve) => {
    const proc = spawn(join(env.bin, 'bash'), [join(env.pkg, 'run.sh'), ...args], {
      cwd: env.work,
      env: { PATH: env.bin, HOME: env.base, STATE: env.state, BIN: env.bin, NO_COLOR: '1', ...extraEnv },
      stdio: [stdin === null ? 'ignore' : 'pipe', 'pipe', 'pipe'],
    });
    let stdout = '', stderr = '';
    proc.stdout.on('data', (d) => { stdout += d; });
    proc.stderr.on('data', (d) => { stderr += d; });
    if (stdin !== null) proc.stdin.end(stdin);
    proc.on('close', (code) => resolve({ code, stdout, stderr, out: stdout + stderr, calls: calls(env) }));
  });
}
const calls = (env) => (existsSync(join(env.state, 'calls.log')) ? readFileSync(join(env.state, 'calls.log'), 'utf8') : '');

const HEALTHY_PY = (v = '3.12') => `#!/bin/sh\necho ${v}\n`;
const BROKEN_PY = '#!/bin/sh\necho "ERROR:root:code for hash blake2b was not found." >&2\necho 3.12\n';

function fakePython(path, body) {
  mkdirSync(dirname(path), { recursive: true });
  writeFileSync(path, body);
  chmodSync(path, 0o755);
  return path;
}

const slugOf = (pkg) => join(pkg, 'tandem').replace(/[^A-Za-z0-9]/g, '-').replace(/^-+/, '');

describe('run.sh happy path', () => {
  it('keeps the tandem virtualenv outside the (possibly read-only) package directory', async () => {
    const env = setup();
    const r = await run(env);
    assert.match(r.stdout, new RegExp(`venv=${env.base}/\\.local/state/tandem/venv`));
    assert.ok(!r.stdout.includes(env.pkg + '/tandem/.venv'));
  });

  it('runs tandem with no prompts when everything is present and current', async () => {
    const env = setup();
    const r = await run(env, ['--nope-flag']);
    assert.equal(r.code, 0, r.out);
    assert.ok(!r.out.includes('[y/N]'));
    assert.match(r.calls, /uv sync --project \S+ --frozen/);
    assert.match(r.stdout, /TANDEM-RAN cwd=.*work args=run --project \S+ tandem --nope-flag/);
  });

  it('accepts letter suffixes (tmux 3.3a) and "next-" tmux builds', async () => {
    for (const tmux of ['tmux 3.3a', 'tmux next-3.5']) {
      const r = await run(setup({ versions: ok({ tmux }) }));
      assert.equal(r.code, 0, tmux + r.out);
    }
  });
});

describe('run.sh missing dependencies', () => {
  it('installs a missing dependency after a yes', async () => {
    const env = setup({ versions: ok({ tmux: null }) });
    const r = await run(env, ['--answers', 'y']);
    assert.equal(r.code, 0, r.out);
    assert.match(r.out, /brew install tmux/);
    assert.match(r.calls, /brew install tmux/);
  });

  it('terminates with 2 on declining a required install', async () => {
    const env = setup({ versions: ok({ tmux: null }) });
    const r = await run(env, ['--answers', 'n']);
    assert.equal(r.code, 2);
    assert.ok(!r.calls.includes('brew install'));
    assert.ok(!r.calls.includes('uv run'));
  });

  it('allows declining agy when claude is installed', async () => {
    const r = await run(setup({ versions: ok({ agy: null }) }), ['--answers', 'n']);
    assert.equal(r.code, 0, r.out);
    assert.match(r.out, /agy.*skipped|skipp.*agy/i);
  });

  it('terminates with 2 when no AI CLI would remain', async () => {
    const r = await run(setup({ versions: ok({ agy: null, claude: null }) }), ['--answers', 'n,n']);
    assert.equal(r.code, 2);
    assert.ok(!r.calls.includes('uv run'));
  });

  it('shows the official installer URL before running a curl installer', async () => {
    const env = setup({ versions: ok({ agy: null }) });
    const r = await run(env, ['--answers', 'y']);
    assert.match(r.out, /curl -fsSL https:\/\/antigravity\.google\/cli\/install\.sh \| bash/);
    assert.equal(r.code, 0, r.out);
  });

  it('exits 4 and shows stderr when the installer fails', async () => {
    const env = setup({ versions: ok({ tmux: null }) });
    env.mark('tmux.installfail');
    const r = await run(env, ['--answers', 'y']);
    assert.equal(r.code, 4);
    assert.match(r.out, /install failed for tmux/);
  });
});

describe('run.sh updates and min-version gate', () => {
  it('soft-prompts for an available update and continues on no', async () => {
    const env = setup();
    env.mark('uv.dryrun', 'Would update uv from v0.10.4 to the latest version\n');
    const r = await run(env, ['--answers', 'n']);
    assert.equal(r.code, 0, r.out);
    assert.match(r.out, /Update uv/);
    assert.ok(!r.calls.includes('uv self update\n'));
  });

  it('applies the update on yes', async () => {
    const env = setup();
    env.mark('uv.dryrun', 'Would update uv from v0.10.4 to the latest version\n');
    const r = await run(env, ['--answers', 'y']);
    assert.equal(r.code, 0, r.out);
    assert.match(r.calls, /^uv self update$/m);
  });

  it('detects brew-outdated system packages', async () => {
    const env = setup();
    env.mark('outdated', 'tmux\n');
    const r = await run(env, ['--answers', 'y']);
    assert.match(r.calls, /brew upgrade tmux/);
  });

  it('skips the soft prompt and says so when the update check fails (offline)', async () => {
    const env = setup();
    env.mark('uv.dryrun.fail');
    const r = await run(env);
    assert.equal(r.code, 0, r.out);
    assert.ok(!r.out.includes('[y/N]'));
    assert.match(r.out, /latest unknown/i);
  });

  it('terminates with 3 when declining an update below the minimum', async () => {
    const env = setup({ versions: ok({ agy: '1.2.0' }) });
    const r = await run(env, ['--answers', 'n']);
    assert.equal(r.code, 3);
    assert.match(r.out, /may crash/);
    assert.ok(!r.calls.includes('uv run'));
  });

  it('updates a below-minimum tool on yes and continues', async () => {
    const env = setup({ versions: ok({ agy: '1.2.0' }) });
    const r = await run(env, ['--answers', 'y']);
    assert.equal(r.code, 0, r.out);
    assert.match(r.calls, /^agy update$/m);
  });

  it('exits 3 when the update does not reach the minimum', async () => {
    const env = setup({ versions: ok({ agy: '1.2.0' }) });
    env.mark('agy.noupdate');
    const r = await run(env, ['--answers', 'y']);
    assert.equal(r.code, 3);
  });

  it('compares numerically, not lexically (2.10 >= 2.9)', async () => {
    const min = MIN.replace('TANDEM_MIN_GIT=2.30', 'TANDEM_MIN_GIT=2.9');
    const r = await run(setup({ min, versions: ok({ git: 'git version 2.10.0' }) }));
    assert.equal(r.code, 0, r.out);
  });
});

describe('run.sh flags and environment', () => {
  it('--check changes nothing and runs nothing', async () => {
    const env = setup({ versions: ok({ tmux: null }) });
    const r = await run(env, ['--check']);
    assert.notEqual(r.code, 0);
    assert.match(r.out, /tmux.*not installed/);
    assert.ok(!r.calls.includes('install'));
    assert.ok(!r.calls.includes('uv run'));
  });

  it('--check exits 0 when ready', async () => {
    const r = await run(setup(), ['--check']);
    assert.equal(r.code, 0, r.out);
    assert.ok(!r.calls.includes('uv sync'));
  });

  it('--yes answers every prompt yes, --no answers no', async () => {
    const yes = await run(setup({ versions: ok({ tmux: null }) }), ['--yes']);
    assert.equal(yes.code, 0, yes.out);
    const no = await run(setup({ versions: ok({ tmux: null }) }), ['--no']);
    assert.equal(no.code, 2);
  });

  it('exits 6 instead of hanging when stdin is not a terminal and no flag is given', async () => {
    const r = await run(setup({ versions: ok({ tmux: null }) }));
    assert.equal(r.code, 6);
    assert.match(r.out, /--yes|--no/);
  });

  it('never reads prompt answers from a piped stdin', async () => {
    const r = await run(setup({ versions: ok({ tmux: null }) }), [], { stdin: 'y\n' });
    assert.equal(r.code, 6);
  });

  it('exits 5 on an unsupported OS', async () => {
    const r = await run(setup({ os: 'Plan9' }));
    assert.equal(r.code, 5);
  });

  it('exits 5 on macOS without Homebrew', async () => {
    const r = await run(setup({ managers: [] }));
    assert.equal(r.code, 5);
    assert.match(r.out, /Homebrew/);
  });

  it('uses sudo visibly with apt-get on Linux', async () => {
    const env = setup({ os: 'Linux', managers: ['apt-get'], versions: ok({ tmux: null }) });
    const r = await run(env, ['--answers', 'y']);
    assert.equal(r.code, 0, r.out);
    assert.match(r.out, /sudo apt-get install -y tmux/);
  });

  it('uses dnf on Linux when apt-get is absent', async () => {
    const env = setup({ os: 'Linux', managers: ['dnf'], versions: ok({ tmux: null }) });
    const r = await run(env, ['--answers', 'y']);
    assert.match(r.out, /sudo dnf install -y tmux/);
  });
});

describe('run.sh security', () => {
  for (const line of ['X=$(touch pwned)', 'X=`touch pwned`', 'X=1.0; touch pwned', 'X=${HOME}', 'lower=1.0', 'X=1.0 # c']) {
    it(`rejects malicious min-versions line ${JSON.stringify(line)} without executing it`, async () => {
      const env = setup({ min: MIN + line + '\n' });
      const r = await run(env);
      assert.equal(r.code, 4, r.out);
      assert.match(r.out, /min-versions\.env/);
      assert.ok(!existsSync(join(env.work, 'pwned')) && !existsSync(join(env.pkg, 'pwned')));
      assert.ok(!r.calls.includes('uv run'));
    });
  }

  it('does not pipe an installer without showing it first (no auto-yes)', async () => {
    const env = setup({ versions: ok({ claude: null }) });
    const r = await run(env, ['--answers', 'n']);
    assert.ok(!r.calls.includes('curl'));
  });

  it('sets HOMEBREW_NO_AUTO_UPDATE for its own checks', () => {
    assert.match(readFileSync(join(ROOT, 'run.sh'), 'utf8'), /HOMEBREW_NO_AUTO_UPDATE=1/);
  });
});

describe('run.sh per-workspace virtualenv', () => {
  it('gives every workspace its own virtualenv so editable installs never collide', async () => {
    const a = setup(), b = setup();
    const ra = await run(a), rb = await run(b);
    const venv = (r) => r.stdout.match(/venv=(\S+)/)[1];
    assert.equal(venv(ra), `${a.base}/.local/state/tandem/venv-${slugOf(a.pkg)}`);
    assert.equal(venv(rb), `${b.base}/.local/state/tandem/venv-${slugOf(b.pkg)}`);
    assert.notEqual(venv(ra).split('/').pop(), venv(rb).split('/').pop());
  });

  it('honours an explicit UV_PROJECT_ENVIRONMENT', async () => {
    const env = setup();
    const r = await run(env, [], { extraEnv: { UV_PROJECT_ENVIRONMENT: join(env.base, 'mine') } });
    assert.match(r.stdout, new RegExp(`venv=${env.base}/mine `));
  });
});

describe('run.sh healthy Python selection', () => {
  const pythonOf = (r) => r.stdout.match(/python=(\S*)/)[1];

  it('uses the Python on PATH when it is healthy', async () => {
    const env = setup();
    fakePython(join(env.bin, 'python3'), HEALTHY_PY());
    const r = await run(env);
    assert.equal(pythonOf(r), join(env.bin, 'python3'));
    assert.match(r.calls, new RegExp(`sync-python=${join(env.bin, 'python3')}`));
  });

  it('skips a Python whose hashlib prints blake2 errors and uses a fallback', async () => {
    const env = setup();
    fakePython(join(env.bin, 'python3'), BROKEN_PY);
    const fallback = fakePython(join(env.base, 'fallback/python3'), HEALTHY_PY());
    const r = await run(env, [], { extraEnv: { TANDEM_PYTHON_FALLBACKS: fallback } });
    assert.equal(r.code, 0, r.out);
    assert.equal(pythonOf(r), fallback);
    assert.match(r.stderr, /broken hashlib/);
  });

  it('skips a Python older than TANDEM_MIN_PYTHON', async () => {
    const env = setup();
    fakePython(join(env.bin, 'python3'), HEALTHY_PY('3.9'));
    const fallback = fakePython(join(env.base, 'fallback/python3'), HEALTHY_PY('3.13'));
    const r = await run(env, [], { extraEnv: { TANDEM_PYTHON_FALLBACKS: fallback } });
    assert.equal(pythonOf(r), fallback);
  });

  it('lets uv choose when no healthy Python is found', async () => {
    const env = setup();
    fakePython(join(env.bin, 'python3'), BROKEN_PY);
    const r = await run(env, [], { extraEnv: { TANDEM_PYTHON_FALLBACKS: join(env.base, 'nowhere/python3') } });
    assert.equal(r.code, 0, r.out);
    assert.equal(pythonOf(r), '');
  });
});

function setupManifest(env, version = '1.2.0') {
  const dir = join(env.pkg, '.agent-workspace');
  mkdirSync(dir, { recursive: true });
  writeFileSync(join(dir, 'manifest.json'), JSON.stringify({ version, managedFiles: [] }, null, 2) + '\n');
}

function setupNpx(env, remoteVersion = '1.3.0') {
  symlinkSync(join(env.state, 'tool.sh'), join(env.bin, 'npx'));
  writeFileSync(join(env.state, 'npx.version'), remoteVersion + '\n');
}

function setupChangelog(env, fromV, toV, extraBullet) {
  const dir = join(env.pkg, '.agent-workspace', 'tandem');
  mkdirSync(dir, { recursive: true });
  writeFileSync(join(dir, 'CHANGELOG.md'), [
    '# Changelog', '',
    `## [${toV}] - 2026-10-01`, '',
    '### Added',
    '- Clickable status-bar buttons',
    '- run.sh updates the workspace on launch',
    ...(extraBullet ? [`- ${extraBullet}`] : []),
    '',
    `## [${fromV}] - 2026-09-01`, '',
    '### Added',
    '- Initial release', '',
  ].join('\n'));
  const srcMin = join(env.pkg, 'tandem', 'min-versions.env');
  writeFileSync(join(dir, 'min-versions.env'), readFileSync(srcMin, 'utf8'));
}

function readNotice(env) {
  const p = join(env.pkg, '.agent-workspace', 'update-notice.env');
  return existsSync(p) ? readFileSync(p, 'utf8') : null;
}

describe('run.sh auto-update', () => {
  it('prints "up to date" when local version matches remote', async () => {
    const env = setup();
    setupManifest(env, '1.3.0');
    setupNpx(env, '1.3.0');
    const r = await run(env);
    assert.equal(r.code, 0, r.out);
    assert.match(r.out, /agent-workspace 1\.3\.0 is up to date/);
    assert.ok(!r.calls.includes('update --yes'));
    assert.ok(r.stdout.includes('TANDEM-RAN'));
  });

  it('applies the update with exact args, writes notice, prints what\'s new, re-execs once', async () => {
    const env = setup();
    setupManifest(env, '1.2.0');
    setupNpx(env, '1.3.0');
    setupChangelog(env, '1.2.0', '1.3.0');
    const r = await run(env);
    assert.equal(r.code, 0, r.out);
    assert.match(r.calls, /npx --yes \S+ update --yes --no-link --no-update-clis --no-commit/);
    assert.equal((r.calls.match(/npx .* update --yes/g) || []).length, 1, 're-exec does not repeat the update');
    const notice = readNotice(env);
    assert.ok(notice !== null, 'update-notice.env written');
    assert.match(notice, /TANDEM_UPDATE_FROM=1\.2\.0/);
    assert.match(notice, /TANDEM_UPDATE_TO=1\.3\.0/);
    assert.match(notice, /TANDEM_UPDATE_AT=\d+/);
    assert.match(r.out, /What's new/);
    assert.match(r.out, /Clickable status-bar buttons/);
    assert.ok(r.stdout.includes('TANDEM-RAN'), 're-exec ran tandem');
  });

  it('truncates long what\'s-new headlines so the box stays aligned', async () => {
    const env = setup();
    setupManifest(env, '1.2.0');
    setupNpx(env, '1.3.0');
    setupChangelog(env, '1.2.0', '1.3.0', 'x'.repeat(120));
    const r = await run(env);
    assert.equal(r.code, 0, r.out);
    assert.ok(!r.out.includes('x'.repeat(65)), 'headline is cut to the box width');
    assert.match(r.out, /x{63}…/);
  });

  it('prints "offline" message and continues on version check failure', async () => {
    const env = setup();
    setupManifest(env, '1.2.0');
    setupNpx(env, '1.3.0');
    env.mark('npx.offline');
    const r = await run(env);
    assert.equal(r.code, 0, r.out);
    assert.match(r.out, /Could not check for updates.*offline/);
    assert.ok(!r.calls.includes('update --yes'));
    assert.ok(r.stdout.includes('TANDEM-RAN'));
  });

  it('warns and continues when the update command fails', async () => {
    const env = setup();
    setupManifest(env, '1.2.0');
    setupNpx(env, '1.3.0');
    env.mark('npx.updatefail');
    const r = await run(env);
    assert.equal(r.code, 0, r.out);
    assert.match(r.out, /Update failed/);
    assert.ok(readNotice(env) === null, 'notice not written on failure');
    assert.ok(r.stdout.includes('TANDEM-RAN'));
  });

  it('skips the update when --no-update is passed', async () => {
    const env = setup();
    setupManifest(env, '1.2.0');
    setupNpx(env, '1.3.0');
    const r = await run(env, ['--no-update']);
    assert.equal(r.code, 0, r.out);
    assert.ok(!r.calls.includes('npx'));
    assert.ok(r.stdout.includes('TANDEM-RAN'));
  });

  it('skips the update and prints a notice when TANDEM_AUTO_UPDATE=0', async () => {
    const env = setup();
    setupManifest(env, '1.2.0');
    setupNpx(env, '1.3.0');
    const r = await run(env, [], { extraEnv: { TANDEM_AUTO_UPDATE: '0' } });
    assert.equal(r.code, 0, r.out);
    assert.match(r.out, /Auto-update off/);
    assert.ok(!r.calls.includes('npx'));
    assert.ok(r.stdout.includes('TANDEM-RAN'));
  });

  it('skips silently when manifest.json is missing (dev checkout)', async () => {
    const env = setup();
    setupNpx(env, '1.3.0');
    const r = await run(env);
    assert.equal(r.code, 0, r.out);
    assert.ok(!r.calls.includes('npx'));
    assert.ok(r.stdout.includes('TANDEM-RAN'));
  });

  it('prints "npx not found" and continues when npx is not on PATH', async () => {
    const env = setup();
    setupManifest(env, '1.2.0');
    const r = await run(env);
    assert.equal(r.code, 0, r.out);
    assert.match(r.out, /npx not found/);
    assert.ok(r.stdout.includes('TANDEM-RAN'));
  });

  it('rejects an invalid TANDEM_UPDATE_SPEC without passing it to npx', async () => {
    const env = setup();
    setupManifest(env, '1.2.0');
    setupNpx(env, '1.3.0');
    const r = await run(env, [], { extraEnv: { TANDEM_UPDATE_SPEC: '; touch pwned' } });
    assert.equal(r.code, 0, r.out);
    assert.ok(!r.calls.includes('npx'), 'npx never called for invalid spec');
    assert.ok(!existsSync(join(env.work, 'pwned')), 'command injection prevented');
    assert.ok(r.stdout.includes('TANDEM-RAN'));
  });

  it('--check reports update available without applying it or writing a notice', async () => {
    const env = setup();
    setupManifest(env, '1.2.0');
    setupNpx(env, '1.3.0');
    const r = await run(env, ['--check']);
    assert.equal(r.code, 0, r.out);
    assert.match(r.out, /↑ agent-workspace 1\.2\.0 .* 1\.3\.0 available/);
    assert.ok(!r.calls.includes('update --yes'), 'no update run during --check');
    assert.ok(readNotice(env) === null, 'notice not written during --check');
  });
});

describe('run.sh broken virtualenv recovery', () => {
  const venvOf = (env) => join(env.base, '.local/state/tandem', `venv-${slugOf(env.pkg)}`);
  const makeVenv = (dir, body) => {
    writeFileSync(join(mkdirSyncReturn(dir), 'pyvenv.cfg'), 'home = /x\n');
    fakePython(join(dir, 'bin/python'), body);
  };
  const mkdirSyncReturn = (dir) => { mkdirSync(dir, { recursive: true }); return dir; };

  it('removes a virtualenv whose Python cannot import hashlib so uv rebuilds it', async () => {
    const env = setup();
    makeVenv(venvOf(env), BROKEN_PY);
    await run(env);
    assert.ok(!existsSync(venvOf(env)));
  });

  it('removes a broken legacy shared virtualenv', async () => {
    const env = setup();
    const legacy = join(env.base, '.local/state/tandem/venv');
    makeVenv(legacy, BROKEN_PY);
    await run(env);
    assert.ok(!existsSync(legacy));
  });

  it('keeps a healthy virtualenv', async () => {
    const env = setup();
    makeVenv(venvOf(env), HEALTHY_PY());
    await run(env);
    assert.ok(existsSync(join(venvOf(env), 'bin/python')));
  });

  it('never deletes a directory that is not a virtualenv', async () => {
    const env = setup();
    const dir = venvOf(env);
    fakePython(join(dir, 'bin/python'), BROKEN_PY);
    await run(env);
    assert.ok(existsSync(join(dir, 'bin/python')));
  });

  it('does not touch virtualenvs under --check', async () => {
    const env = setup();
    makeVenv(venvOf(env), BROKEN_PY);
    await run(env, ['--check']);
    assert.ok(existsSync(venvOf(env)));
  });
});
