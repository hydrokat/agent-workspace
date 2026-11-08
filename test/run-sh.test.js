import { describe, it, afterEach } from 'node:test';
import assert from 'node:assert/strict';
import { spawn, execFileSync } from 'node:child_process';
import { chmodSync, copyFileSync, mkdirSync, readFileSync, existsSync, rmSync, symlinkSync, writeFileSync } from 'node:fs';
import { join, dirname } from 'node:path';
import { fileURLToPath } from 'node:url';
import { makeTmp, cleanup } from './helpers.js';

const ROOT = dirname(dirname(fileURLToPath(import.meta.url)));
const CORE = ['sed', 'grep', 'cat', 'tr', 'mktemp', 'sleep', 'kill', 'cut', 'dirname', 'basename', 'rm', 'ln', 'sh', 'head', 'date', 'mv', 'tar', 'mkdir', 'cp', 'unzip', 'gzip', 'chmod'];
const MIN = [
  '# test minimums', 'TANDEM_MIN_CARGO=1.89', 'TANDEM_MIN_GIT=2.30', 'TANDEM_MIN_TMUX=3.3',
  'TANDEM_MIN_CLAUDE=2.1.301', 'TANDEM_MIN_AGY=1.3.6', '',
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
      outdated) [ -f "$STATE/brew.outdated.fail" ] && exit 1; [ -f "$STATE/outdated" ] && cat "$STATE/outdated"; exit 0;;
    esac;;
  tmux)
    case "$*" in
      *has-session*)
        if [ -f "$STATE/tmux.running" ]; then exit 0; fi
        exit 1
        ;;
    esac;;
  sudo) exec "$@";;
  pacman)
    case "$1" in
      -S) shift; if [ "$1" = "--noconfirm" ]; then shift; fi; install_pkg "$1"; exit 0;;
      -Syu) shift; if [ "$1" = "--noconfirm" ]; then shift; fi; bump "$1"; exit 0;;
      -Qu)
        if [ -f "$STATE/outdated" ] && grep -q "^$2 " "$STATE/outdated"; then
          grep "^$2 " "$STATE/outdated"
          exit 0
        fi
        exit 1;;
    esac;;
  apt-get|dnf) if [ "$1" = install ] || [ "$2" = install ]; then for last; do :; done; install_pkg "$last"; fi; exit 0;;
  curl)
    out_file=""
    url=""
    for arg in "$@"; do
      case "$arg" in
        http*) url="$arg" ;;
      esac
    done
    prev=""
    for arg in "$@"; do
      if [ "$prev" = "-o" ]; then out_file="$arg"; fi
      prev="$arg"
    done
    if [ -n "$url" ]; then
      case "$url" in
        *claude.ai*) n=claude ;;
        *antigravity*) n=agy ;;
        *rustup*) n=cargo ;;
        *releases/download*SHA256SUMS*)
          [ -f "$STATE/curl.404" ] && exit 22
          if [ -f "$STATE/SHA256SUMS" ]; then
            if [ -n "$out_file" ]; then cp "$STATE/SHA256SUMS" "$out_file"; else cat "$STATE/SHA256SUMS"; fi
            exit 0
          fi
          exit 22
          ;;
        *releases/download*.tar.gz*)
          [ -f "$STATE/curl.404" ] && exit 22
          if [ -f "$STATE/tandem-asset.tar.gz" ]; then
            if [ -n "$out_file" ]; then cp "$STATE/tandem-asset.tar.gz" "$out_file"; else cat "$STATE/tandem-asset.tar.gz"; fi
            exit 0
          fi
          exit 22
          ;;
        *releases/download*.zip*)
          [ -f "$STATE/curl.404" ] && exit 22
          if [ -f "$STATE/tandem-asset.zip" ]; then
            if [ -n "$out_file" ]; then cp "$STATE/tandem-asset.zip" "$out_file"; else cat "$STATE/tandem-asset.zip"; fi
            exit 0
          fi
          exit 22
          ;;
        *) exit 22 ;;
      esac
      [ -f "$STATE/$n.installfail" ] && exit 22
      echo "ln -sf \\"$STATE/tool.sh\\" \\"$BIN/$n\\"; echo \\"$n 9.9.9\\" > \\"$STATE/$n.version\\""
      exit 0
    fi
    exit 22 ;;
  rustup)
    if [ "$1" = update ]; then [ -f "$STATE/rustup.noupdate" ] || bump cargo; exit 0; fi;;
  cargo)
    case "$1" in
      build)
        [ -f "$STATE/cargo.buildfail" ] && { echo "error: could not compile tandem" >&2; exit 101; }
        td=""; prev=""
        for arg in "$@"; do [ "$prev" = "--target-dir" ] && td="$arg"; prev="$arg"; done
        mkdir -p "$td/release"
        ver="1.4.0"; [ -f "$STATE/cargo.builtversion" ] && ver=$(cat "$STATE/cargo.builtversion")
        printf '#!/bin/sh\\ncase "$1" in --version) echo "tandem %s";; *) echo "TANDEM-SOURCE-RAN cwd=$PWD args=$* self=$TANDEM_SELF";; esac\\n' "$ver" > "$td/release/tandem"
        chmod +x "$td/release/tandem"
        cp "$td/release/tandem" "$td/release/tandem.exe"
        exit 0;;
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

const ok = (v) => ({ git: 'git version 2.54.0 (Apple Git-157)', tmux: 'tmux 3.3a', cargo: 'cargo 1.89.0 (abc123def 2026-08-07)', rustup: 'rustup 1.28.2',
  claude: '2.1.301 (Claude Code)', agy: '1.3.6', ...v });

let dirs = [];
afterEach(() => { dirs.forEach(cleanup); dirs = []; });

const CARGO_TOML = (v) => `[package]\nname = "tandem"\nversion = "${v}"\nedition = "2024"\n\n[dependencies]\nserde = { version = "9.9.9" }\n`;

const slugFor = (os, arch) => {
  const family = os.startsWith('Darwin') ? 'darwin' : os.startsWith('Linux') ? 'linux' : /^(MINGW|MSYS|CYGWIN)/.test(os) ? 'windows' : null;
  const cpu = arch === 'arm64' || arch === 'aarch64' ? 'arm64' : arch === 'x86_64' ? 'x86_64' : null;
  return family && cpu ? `${family}-${cpu}` : null;
};

function setup({ versions = ok(), os = 'Darwin', arch = 'arm64', managers = ['brew'], min = MIN, cached = true } = {}) {
  const base = makeTmp();
  dirs.push(base);
  const pkg = join(base, 'pkg'), state = join(base, 'state'), bin = join(base, 'bin'), work = join(base, 'work');
  for (const d of [join(pkg, 'tandem'), state, bin, work]) mkdirSync(d, { recursive: true });
  copyFileSync(join(ROOT, 'run.sh'), join(pkg, 'run.sh'));
  writeFileSync(join(pkg, 'tandem/min-versions.env'), min);
  writeFileSync(join(state, 'tool.sh'), TOOL_SH);
  chmodSync(join(state, 'tool.sh'), 0o755);
  writeFileSync(join(pkg, 'tandem/Cargo.toml'), CARGO_TOML('1.4.0'));
  for (const c of CORE) symlinkSync(execFileSync('/usr/bin/which', [c]).toString().trim(), join(bin, c));
  for (const c of ['shasum', 'sha256sum']) {
    try {
      const p = execFileSync('/usr/bin/which', [c]).toString().trim();
      if (p) symlinkSync(p, join(bin, c));
    } catch {}
  }
  symlinkSync(execFileSync('/usr/bin/which', ['bash']).toString().trim(), join(bin, 'bash'));
  writeFileSync(join(bin, 'uname'), `#!/bin/sh\ncase "$1" in -m) echo "${arch}";; *) echo "${os}";; esac\n`);
  chmodSync(join(bin, 'uname'), 0o755);
  const link = (n) => symlinkSync(join(state, 'tool.sh'), join(bin, n));
  for (const [n, banner] of Object.entries(versions)) {
    if (banner === null) continue;
    link(n);
    writeFileSync(join(state, `${n}.version`), banner + '\n');
  }
  for (const m of [...managers, 'curl']) link(m);
  if (os === 'Linux') link('sudo');
  const env = { base, pkg, state, bin, work, mark: (f, c = '') => writeFileSync(join(state, f), c) };
  if (cached && slugFor(os, arch)) makeCachedBinary(env, '1.4.0', slugFor(os, arch));
  return env;
}

const setupSource = (options = {}) => setup({ ...options, cached: false });

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

describe('run.sh happy path', () => {
  it('builds into a state directory outside the (possibly read-only) package directory', async () => {
    const env = setupSource();
    const r = await run(env, ['--yes']);
    assert.match(r.calls, new RegExp(`cargo build --release --locked --manifest-path ${env.pkg}/tandem/Cargo\\.toml --target-dir ${env.base}/\\.local/state/tandem/build-`));
    assert.ok(!r.calls.includes(`--target-dir ${env.pkg}`));
  });

  it('runs tandem with no prompts when everything is present and current', async () => {
    const env = setup();
    const r = await run(env, ['--nope-flag']);
    assert.equal(r.code, 0, r.out);
    assert.ok(!r.out.includes('[y/N]'));
    assert.ok(!r.calls.includes('cargo build'));
    assert.match(r.stdout, /TANDEM-BINARY-RAN args=--nope-flag/);
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
    assert.ok(!r.calls.includes('cargo build'));
  });

  it('allows declining agy when claude is installed', async () => {
    const r = await run(setup({ versions: ok({ agy: null }) }), ['--answers', 'n']);
    assert.equal(r.code, 0, r.out);
    assert.match(r.out, /agy.*skipped|skipp.*agy/i);
  });

  it('terminates with 2 when no AI CLI would remain', async () => {
    const r = await run(setup({ versions: ok({ agy: null, claude: null }) }), ['--answers', 'n,n']);
    assert.equal(r.code, 2);
    assert.ok(!r.calls.includes('cargo build'));
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
    env.mark('outdated', 'tmux\n');
    const r = await run(env, ['--answers', 'n']);
    assert.equal(r.code, 0, r.out);
    assert.match(r.out, /Update tmux/);
    assert.ok(!r.calls.includes('brew upgrade'));
  });

  it('detects brew-outdated system packages', async () => {
    const env = setup();
    env.mark('outdated', 'tmux\n');
    const r = await run(env, ['--answers', 'y']);
    assert.match(r.calls, /brew upgrade tmux/);
  });

  it('skips the soft prompt and says so when the update check fails (offline)', async () => {
    const env = setup();
    env.mark('brew.outdated.fail');
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
    assert.ok(!r.calls.includes('cargo build'));
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
    assert.ok(!r.calls.includes('cargo build'));
  });

  it('--check exits 0 when ready', async () => {
    const r = await run(setup(), ['--check']);
    assert.equal(r.code, 0, r.out);
    assert.ok(!r.calls.includes('cargo build'));
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
      assert.ok(!r.calls.includes('cargo build'));
    });
  }

  it('does not pipe an installer without showing it first (no auto-yes)', async () => {
    const env = setup({ versions: ok({ claude: null }) });
    const r = await run(env, ['--answers', 'n']);
    assert.ok(!r.calls.includes('claude.ai'));
  });

  it('sets HOMEBREW_NO_AUTO_UPDATE for its own checks', () => {
    assert.match(readFileSync(join(ROOT, 'run.sh'), 'utf8'), /HOMEBREW_NO_AUTO_UPDATE=1/);
  });
});

describe('run.sh source fallback', () => {
  const stateOf = (env) => join(env.base, '.local/state/tandem');

  it('gives every workspace its own build directory', async () => {
    const a = setupSource(), b = setupSource();
    const ra = await run(a, ['--yes']), rb = await run(b, ['--yes']);
    const target = (r) => r.calls.match(/--target-dir (\S+)/)[1];
    assert.notEqual(target(ra), target(rb));
    assert.match(target(ra), new RegExp(`^${a.base}/\\.local/state/tandem/build-`));
  });

  it('honours XDG_STATE_HOME for the build directory', async () => {
    const env = setupSource();
    const xdg = join(env.base, 'xdg');
    const r = await run(env, ['--yes'], { extraEnv: { XDG_STATE_HOME: xdg } });
    assert.match(r.calls, new RegExp(`--target-dir ${xdg}/tandem/build-`));
  });

  it('reports the runtime as source and passes arguments through', async () => {
    const r = await run(setupSource(), ['--yes', '--', '--flag', 'x']);
    assert.equal(r.code, 0, r.out);
    assert.match(r.stdout, /runtime: source/);
    assert.match(r.stdout, /args=--flag x /);
  });

  it('never needs uv or Python', async () => {
    const r = await run(setupSource(), ['--yes']);
    assert.equal(r.code, 0, r.out);
    assert.doesNotMatch(r.out, /\buv\b|python/i);
  });

  it('reads the version from [package], not from a dependency', async () => {
    const env = setupSource();
    writeFileSync(join(env.pkg, 'tandem/Cargo.toml'), CARGO_TOML('2.0.1'));
    env.mark('cargo.builtversion', '2.0.1');
    const r = await run(env, ['--yes']);
    assert.equal(r.code, 0, r.out);
  });

  it('exits 4 when the built binary reports a different version', async () => {
    const env = setupSource();
    env.mark('cargo.builtversion', '9.9.9');
    const r = await run(env, ['--yes']);
    assert.equal(r.code, 4);
    assert.match(r.out, /Unexpected --version output/);
  });

  it('exits 4 when cargo build fails', async () => {
    const env = setupSource();
    env.mark('cargo.buildfail');
    const r = await run(env, ['--yes']);
    assert.equal(r.code, 4);
    assert.match(r.out, /cargo build failed/);
  });

  it('exits 4 when the crate sources are missing', async () => {
    const env = setupSource();
    rmSync(join(env.pkg, 'tandem/Cargo.toml'));
    writeFileSync(join(env.pkg, 'package.json'), '{ "version": "1.4.0" }\n');
    const r = await run(env, ['--yes']);
    assert.equal(r.code, 4);
    assert.match(r.out, /sources not found/);
    assert.ok(!r.calls.includes('cargo build'));
  });

  it('offers to install cargo through rustup and shows the command first', async () => {
    const env = setupSource({ versions: ok({ cargo: null, rustup: null }) });
    const r = await run(env, ['--answers', 'y,y']);
    assert.equal(r.code, 0, r.out);
    assert.match(r.out, /curl --proto '=https' --tlsv1\.2 -sSf https:\/\/sh\.rustup\.rs \| sh -s -- -y --profile minimal/);
    assert.match(r.calls, /^cargo build /m);
  });

  it('terminates with 2 when cargo is needed and the install is declined', async () => {
    const env = setupSource({ versions: ok({ cargo: null, rustup: null }) });
    const r = await run(env, ['--answers', 'y,n']);
    assert.equal(r.code, 2);
    assert.match(r.out, /sh\.rustup\.rs/);
    assert.ok(!r.calls.includes('cargo build'));
  });

  it('terminates with 3 when cargo is below the minimum and the update is declined', async () => {
    const env = setupSource({ versions: ok({ cargo: 'cargo 1.70.0 (abc 2023-06-01)' }) });
    const r = await run(env, ['--answers', 'y,n']);
    assert.equal(r.code, 3);
    assert.ok(!r.calls.includes('cargo build'));
  });

  it('updates a below-minimum cargo through rustup and builds', async () => {
    const env = setupSource({ versions: ok({ cargo: 'cargo 1.70.0 (abc 2023-06-01)' }) });
    const r = await run(env, ['--answers', 'y,y']);
    assert.equal(r.code, 0, r.out);
    assert.match(r.calls, /^rustup update$/m);
    assert.match(r.calls, /^cargo build /m);
  });

  it('does not require cargo when a healthy binary is cached', async () => {
    const env = setupSource({ versions: ok({ cargo: null, rustup: null }) });
    makeCachedBinary(env);
    const r = await run(env, ['--yes']);
    assert.equal(r.code, 0, r.out);
    assert.doesNotMatch(r.stdout, /cargo/);
  });

  it('--check treats a missing cargo as optional while the binary is simply not cached yet', async () => {
    const env = setupSource({ versions: ok({ cargo: null, rustup: null }) });
    const r = await run(env, ['--check']);
    assert.equal(r.code, 0, r.out);
    assert.match(r.stdout, /cargo.*only needed if the binary download fails/);
    assert.match(r.stdout, /runtime: source \(binary not cached\)/);
  });

  it('--check still requires cargo when the source runtime was chosen explicitly', async () => {
    const env = setupSource({ versions: ok({ cargo: null, rustup: null }) });
    const r = await run(env, ['--check'], { extraEnv: { TANDEM_RUNTIME: 'source' } });
    assert.equal(r.code, 2);
    assert.match(r.stdout, /cargo.*not installed \(required\)/);
  });

  describe('consent before building on the user machine', () => {
    const buildDirOf = (env) => join(env.base, '.local/state/tandem', `build-${join(env.pkg, 'tandem').replace(/[^A-Za-z0-9]/g, '-').replace(/^-+/, '')}`);

    it('asks before compiling when no usable prebuilt binary exists, and says why', async () => {
      const env = setupSource();
      env.mark('curl.404', '1');
      const r = await run(env, ['--answers', 'y']);
      assert.equal(r.code, 0, r.out);
      assert.match(r.out, /Build tandem from source\?/);
      assert.match(r.out, /download failed/);
      assert.match(r.out, /cargo build --release --locked/);
      assert.match(r.calls, /^cargo build /m);
    });

    it('does not compile anything when the build is declined, and exits 2', async () => {
      const env = setupSource();
      const r = await run(env, ['--no']);
      assert.equal(r.code, 2);
      assert.match(r.out, /Build tandem from source\?/);
      assert.match(r.out, /declined/i);
      assert.ok(!r.calls.includes('cargo build'));
    });

    it('never builds silently when stdin is not a terminal (exit 6)', async () => {
      const r = await run(setupSource());
      assert.equal(r.code, 6);
      assert.ok(!r.calls.includes('cargo build'));
    });

    it('--yes consents to the build', async () => {
      const r = await run(setupSource(), ['--yes']);
      assert.equal(r.code, 0, r.out);
      assert.match(r.calls, /^cargo build /m);
    });

    it('asks about the build before it asks to install cargo, and installs nothing when declined', async () => {
      const env = setupSource({ versions: ok({ cargo: null, rustup: null }) });
      const declined = await run(env, ['--answers', 'n']);
      assert.equal(declined.code, 2);
      assert.ok(!declined.out.includes('rustup'), declined.out);

      const env2 = setupSource({ versions: ok({ cargo: null, rustup: null }) });
      const accepted = await run(env2, ['--answers', 'y,y']);
      assert.equal(accepted.code, 0, accepted.out);
      assert.ok(accepted.out.indexOf('Build tandem from source?') < accepted.out.indexOf('sh.rustup.rs'), 'build question comes first');
    });

    it('falls back to a consented build on a platform with no prebuilt binary', async () => {
      const env = setupSource({ arch: 'riscv64' });
      const r = await run(env, ['--answers', 'y']);
      assert.equal(r.code, 0, r.out);
      assert.match(r.out, /no prebuilt binary for this platform/);
      assert.match(r.calls, /^cargo build /m);
    });

    it('treats an explicit TANDEM_RUNTIME=source as consent', async () => {
      const r = await run(setupSource(), [], { extraEnv: { TANDEM_RUNTIME: 'source' } });
      assert.equal(r.code, 0, r.out);
      assert.ok(!r.out.includes('Build tandem from source?'));
      assert.match(r.calls, /^cargo build /m);
    });

    it('reuses a healthy local build of the same version without asking or needing cargo', async () => {
      const env = setupSource({ versions: ok({ cargo: null, rustup: null }) });
      const dir = join(buildDirOf(env), 'release');
      mkdirSync(dir, { recursive: true });
      writeFileSync(join(dir, 'tandem'), '#!/bin/sh\ncase "$1" in --version) echo "tandem 1.4.0";; *) echo "REUSED-BUILD";; esac\n');
      chmodSync(join(dir, 'tandem'), 0o755);
      const r = await run(env, ['--yes']);
      assert.equal(r.code, 0, r.out);
      assert.ok(!r.out.includes('[y/N]'));
      assert.ok(!r.calls.includes('cargo build'));
      assert.match(r.stdout, /REUSED-BUILD/);
    });

    it('asks again when the local build is for another version', async () => {
      const env = setupSource();
      const dir = join(buildDirOf(env), 'release');
      mkdirSync(dir, { recursive: true });
      writeFileSync(join(dir, 'tandem'), '#!/bin/sh\necho "tandem 0.0.1"\n');
      chmodSync(join(dir, 'tandem'), 0o755);
      const r = await run(env);
      assert.equal(r.code, 6);
    });

    it('--check never builds and never asks', async () => {
      const r = await run(setupSource(), ['--check']);
      assert.equal(r.code, 0, r.out);
      assert.ok(!r.out.includes('[y/N]'));
      assert.ok(!r.calls.includes('cargo build'));
    });
  });

  describe('legacy virtualenv cleanup', () => {
    const makeVenv = (dir, withCfg = true) => {
      mkdirSync(join(dir, 'bin'), { recursive: true });
      if (withCfg) writeFileSync(join(dir, 'pyvenv.cfg'), 'home = /x\n');
    };

    it('removes venv directories left by the Python tandem', async () => {
      const env = setupSource();
      makeVenv(join(stateOf(env), 'venv'));
      makeVenv(join(stateOf(env), 'venv-some-workspace'));
      await run(env, ['--yes']);
      assert.ok(!existsSync(join(stateOf(env), 'venv')));
      assert.ok(!existsSync(join(stateOf(env), 'venv-some-workspace')));
    });

    it('also cleans up on the binary runtime', async () => {
      const env = setupSource();
      makeCachedBinary(env);
      makeVenv(join(stateOf(env), 'venv-some-workspace'));
      await run(env, ['--yes']);
      assert.ok(!existsSync(join(stateOf(env), 'venv-some-workspace')));
    });

    it('never deletes a directory that is not a virtualenv', async () => {
      const env = setupSource();
      makeVenv(join(stateOf(env), 'venv-notes'), false);
      await run(env, ['--yes']);
      assert.ok(existsSync(join(stateOf(env), 'venv-notes')));
    });

    it('does not touch virtualenvs under --check', async () => {
      const env = setupSource();
      makeVenv(join(stateOf(env), 'venv-some-workspace'));
      await run(env, ['--check']);
      assert.ok(existsSync(join(stateOf(env), 'venv-some-workspace')));
    });
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
  writeFileSync(join(dir, 'Cargo.toml'), CARGO_TOML('1.4.0'));
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
    assert.ok(r.stdout.includes('TANDEM-BINARY-RAN'));
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
    assert.ok(!existsSync(join(env.pkg, '.agent-workspace', 'update-notice.env.tmp')), 'notice written atomically, no temp file left');
    assert.match(r.out, /What's new/);
    assert.match(r.out, /Clickable status-bar buttons/);
    assert.ok(r.stdout.includes('TANDEM-BINARY-RAN'), 're-exec ran tandem');
  });

  it('shows only the bold label of a changelog bullet, without markdown', async () => {
    const env = setup();
    setupManifest(env, '1.2.0');
    setupNpx(env, '1.3.0');
    setupChangelog(env, '1.2.0', '1.3.0', '**Auto-update on launch (`run.sh`):** checks for a newer release before doing anything else');
    const r = await run(env);
    assert.equal(r.code, 0, r.out);
    assert.match(r.out, /• Auto-update on launch \(run\.sh\)\s+│/);
    assert.ok(!r.out.includes('**'), 'bold markers are removed');
    assert.ok(!r.out.includes('before doing anything else'), 'description is dropped');
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
    assert.ok(r.stdout.includes('TANDEM-BINARY-RAN'));
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
    assert.ok(r.stdout.includes('TANDEM-BINARY-RAN'));
  });

  it('skips the update when --no-update is passed', async () => {
    const env = setup();
    setupManifest(env, '1.2.0');
    setupNpx(env, '1.3.0');
    const r = await run(env, ['--no-update']);
    assert.equal(r.code, 0, r.out);
    assert.ok(!r.calls.includes('npx'));
    assert.ok(r.stdout.includes('TANDEM-BINARY-RAN'));
  });

  it('skips the update and prints a notice when TANDEM_AUTO_UPDATE=0', async () => {
    const env = setup();
    setupManifest(env, '1.2.0');
    setupNpx(env, '1.3.0');
    const r = await run(env, [], { extraEnv: { TANDEM_AUTO_UPDATE: '0' } });
    assert.equal(r.code, 0, r.out);
    assert.match(r.out, /Auto-update off/);
    assert.ok(!r.calls.includes('npx'));
    assert.ok(r.stdout.includes('TANDEM-BINARY-RAN'));
  });

  it('skips silently when manifest.json is missing (dev checkout)', async () => {
    const env = setup();
    setupNpx(env, '1.3.0');
    const r = await run(env);
    assert.equal(r.code, 0, r.out);
    assert.ok(!r.calls.includes('npx'));
    assert.ok(r.stdout.includes('TANDEM-BINARY-RAN'));
  });

  it('prints "npx not found" and continues when npx is not on PATH', async () => {
    const env = setup();
    setupManifest(env, '1.2.0');
    const r = await run(env);
    assert.equal(r.code, 0, r.out);
    assert.match(r.out, /npx not found/);
    assert.ok(r.stdout.includes('TANDEM-BINARY-RAN'));
  });

  it('rejects an invalid TANDEM_UPDATE_SPEC without passing it to npx', async () => {
    const env = setup();
    setupManifest(env, '1.2.0');
    setupNpx(env, '1.3.0');
    const r = await run(env, [], { extraEnv: { TANDEM_UPDATE_SPEC: '; touch pwned' } });
    assert.equal(r.code, 0, r.out);
    assert.ok(!r.calls.includes('npx'), 'npx never called for invalid spec');
    assert.ok(!existsSync(join(env.work, 'pwned')), 'command injection prevented');
    assert.ok(r.stdout.includes('TANDEM-BINARY-RAN'));
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

function makeBinaryAsset(stateDir, version = '1.4.0', banner = `tandem ${version}`, slug = 'darwin-arm64') {
  const tmp = makeTmp();
  dirs.push(tmp);
  const tdir = join(tmp, 'tandem');
  mkdirSync(tdir, { recursive: true });
  const isWindows = slug.startsWith('windows');
  const binName = isWindows ? 'tandem.exe' : 'tandem';
  const bin = join(tdir, binName);
  writeFileSync(bin, `#!/bin/sh\ncase "$1" in --version) echo "${banner}"; exit 0;; --help) echo "help"; exit 0;; esac\necho "TANDEM-BINARY-RAN args=$*"\n`);
  chmodSync(bin, 0o755);
  const ext = isWindows ? 'zip' : 'tar.gz';
  const assetPath = join(stateDir, `tandem-asset.${ext}`);
  if (isWindows) {
    execFileSync('zip', ['-q', '-r', assetPath, 'tandem'], { cwd: tmp });
  } else {
    execFileSync('tar', ['-czf', assetPath, '-C', tmp, 'tandem']);
  }
  let sha = '';
  try {
    sha = execFileSync('shasum', ['-a', '256', assetPath]).toString().split(' ')[0];
  } catch {
    sha = execFileSync('sha256sum', [assetPath]).toString().split(' ')[0];
  }
  const assetName = `tandem-${version}-${slug}.${ext}`;
  writeFileSync(join(stateDir, 'SHA256SUMS'), `${sha}  ${assetName}\n`);
}

function makeCachedBinary(env, version = '1.4.0', slug = 'darwin-arm64', banner = `tandem ${version}`) {
  const bdir = join(env.base, '.local/state/tandem/bin', `${version}-${slug}`, 'tandem');
  mkdirSync(bdir, { recursive: true });
  const bin = join(bdir, slug.startsWith('windows') ? 'tandem.exe' : 'tandem');
  writeFileSync(bin, `#!/bin/sh\ncase "$1" in --version) echo "${banner}"; exit 0;; --help) echo "help"; exit 0;; esac\necho "TANDEM-BINARY-RAN args=$*"\n`);
  chmodSync(bin, 0o755);
  writeFileSync(join(env.base, '.local/state/tandem/bin', `${version}-${slug}`, '.managed'), '');
}

describe('run.sh binary runtime selection', () => {

  it('launches cached healthy binary without requiring cargo', async () => {
    const env = setup({ versions: ok({ cargo: null }) });
    makeCachedBinary(env);
    const res = await run(env);
    assert.equal(res.code, 0);
    assert.match(res.stdout, /runtime: binary 1\.4\.0/);
    assert.match(res.stdout, /TANDEM-BINARY-RAN/);
    assert.doesNotMatch(res.stdout, /cargo/);
  });

  it('downloads and verifies binary when not cached', async () => {
    const env = setup({ versions: ok({ cargo: null }) });
    makeBinaryAsset(env.state);
    const res = await run(env);
    assert.equal(res.code, 0);
    assert.match(res.stdout, /runtime: binary 1\.4\.0/);
    assert.match(res.stdout, /TANDEM-BINARY-RAN/);
  });

  it('falls back to a source build when binary download 404s', async () => {
    const env = setupSource();
    env.mark('curl.404', '1');
    const res = await run(env, ['--yes']);
    assert.equal(res.code, 0);
    assert.match(res.stdout, /runtime: source/);
    assert.match(res.stdout, /TANDEM-SOURCE-RAN/);
  });

  it('falls back to a source build on checksum mismatch', async () => {
    const env = setupSource();
    makeBinaryAsset(env.state);
    writeFileSync(join(env.state, 'SHA256SUMS'), '0000000000000000000000000000000000000000000000000000000000000000  tandem-1.4.0-darwin-arm64.tar.gz\n');
    const res = await run(env, ['--yes']);
    assert.equal(res.code, 0);
    assert.match(res.stdout, /runtime: source/);
    assert.match(res.stdout, /TANDEM-SOURCE-RAN/);
  });

  it('falls back to a source build on unhealthy binary', async () => {
    const env = setup();
    makeCachedBinary(env, '1.4.0', 'darwin-arm64', 'tandem 9.9.9');
    const res = await run(env, ['--yes']);
    assert.equal(res.code, 0);
    assert.match(res.stdout, /runtime: source/);
    assert.match(res.stdout, /TANDEM-SOURCE-RAN/);
  });

  it('fails with code 4 when TANDEM_RUNTIME=binary and binary is unavailable', async () => {
    const env = setupSource();
    env.mark('curl.404', '1');
    const res = await run(env, [], { extraEnv: { TANDEM_RUNTIME: 'binary' } });
    assert.equal(res.code, 4);
    assert.match(res.stderr, /Binary runtime unavailable/);
  });

  it('builds from source when TANDEM_RUNTIME=source even if a binary is cached', async () => {
    const env = setup();
    makeCachedBinary(env);
    const res = await run(env, [], { extraEnv: { TANDEM_RUNTIME: 'source' } });
    assert.equal(res.code, 0);
    assert.match(res.stdout, /runtime: source/);
    assert.match(res.stdout, /TANDEM-SOURCE-RAN/);
  });

  it('rejects TANDEM_RUNTIME=python like any unknown runtime', async () => {
    const env = setup();
    const res = await run(env, [], { extraEnv: { TANDEM_RUNTIME: 'python' } });
    assert.equal(res.code, 4);
    assert.match(res.stderr, /must be auto, binary, or source/);
  });

  it('./run.sh --help contains no references to python', () => {
    const out = execFileSync(join(ROOT, 'run.sh'), ['--help'], { encoding: 'utf8' });
    assert.doesNotMatch(out, /python/i);
  });

  it('rejects an unknown TANDEM_RUNTIME value', async () => {
    const res = await run(setup(), [], { extraEnv: { TANDEM_RUNTIME: 'bogus' } });
    assert.equal(res.code, 4);
    assert.match(res.stderr, /must be auto, binary, or source/);
  });

  it('falls back to package.json for the expected version when Cargo.toml is absent', async () => {
    const env = setup();
    rmSync(join(env.pkg, 'tandem/Cargo.toml'));
    writeFileSync(join(env.pkg, 'package.json'), '{ "name": "x", "version": "1.7.0" }\n');
    makeCachedBinary(env, '1.7.0');
    const res = await run(env);
    assert.equal(res.code, 0, res.out);
    assert.match(res.stdout, /runtime: binary 1\.7\.0/);
  });

  it('--check never downloads even if binary not cached', async () => {
    const env = setupSource();
    makeBinaryAsset(env.state);
    const res = await run(env, ['--check']);
    assert.equal(res.code, 0);
    assert.match(res.stdout, /runtime: source \(binary not cached\)/);
    const calls = existsSync(join(env.state, 'calls.log')) ? readFileSync(join(env.state, 'calls.log'), 'utf8') : '';
    assert.doesNotMatch(calls, /releases\/download/);
  });

  it('--fetch-runtime fetches binary and prints TANDEM_SELF without launching', async () => {
    const env = setup();
    makeBinaryAsset(env.state);
    const res = await run(env, ['--fetch-runtime']);
    assert.equal(res.code, 0);
    assert.match(res.stdout, /TANDEM_SELF=/);
    assert.doesNotMatch(res.stdout, /TANDEM-BINARY-RAN/);
  });

  it('--fetch-runtime exits 4 on failure', async () => {
    const env = setupSource();
    env.mark('curl.404', '1');
    const res = await run(env, ['--fetch-runtime']);
    assert.equal(res.code, 4);
  });
});

describe('run.sh Windows (MSYS2/Git Bash) support', () => {
  it('detects MSYS2 with pacman and reports bootstrap banner', async () => {
    const env = setup({ os: 'MINGW64_NT-10.0', arch: 'x86_64', managers: ['pacman'] });
    const res = await run(env, ['--check']);
    assert.equal(res.code, 0);
    assert.match(res.stdout, /tandem bootstrap · MINGW64_NT-10\.0 \(pacman\)/);
  });

  it('exits 5 when pacman is missing with guidance to install MSYS2 or use WSL', async () => {
    const env = setup({ os: 'MINGW64_NT-10.0', arch: 'x86_64', managers: [] });
    const res = await run(env, ['--check']);
    assert.equal(res.code, 5);
    assert.match(res.stderr, /Install MSYS2 and run from its shell, or use WSL/);
  });

  it('installs tools using pacman -S --noconfirm', async () => {
    const env = setup({ os: 'MSYS_NT-10.0', arch: 'x86_64', managers: ['pacman'], versions: ok({ tmux: null }) });
    const res = await run(env, ['--yes']);
    assert.equal(res.code, 0);
    assert.match(res.calls, /pacman -S --noconfirm tmux/);
  });

  it('updates tools using pacman -Syu --noconfirm', async () => {
    const env = setup({ os: 'CYGWIN_NT-10.0', arch: 'x86_64', managers: ['pacman'], versions: ok({ tmux: 'tmux 1.0' }) });
    const res = await run(env, ['--yes']);
    assert.equal(res.code, 0);
    assert.match(res.calls, /pacman -Syu --noconfirm tmux/);
  });

  it('downloads, verifies, and launches windows-x86_64 zip binary (tandem.exe)', async () => {
    const env = setup({ os: 'MINGW64_NT-10.0', arch: 'x86_64', managers: ['pacman'], versions: ok({ cargo: null }) });
    makeBinaryAsset(env.state, '1.4.0', 'tandem 1.4.0', 'windows-x86_64');
    const res = await run(env);
    assert.equal(res.code, 0);
    assert.match(res.stdout, /runtime: binary 1\.4\.0 \(windows-x86_64\)/);
    assert.match(res.stdout, /TANDEM-BINARY-RAN/);
  });

  it('WSL reports Linux and uses Linux binary without regressing', async () => {
    const env = setup({ os: 'Linux', arch: 'x86_64', managers: ['apt-get'], versions: ok({ cargo: null }) });
    makeBinaryAsset(env.state, '1.4.0', 'tandem 1.4.0', 'linux-x86_64');
    const res = await run(env);
    assert.equal(res.code, 0);
    assert.match(res.stdout, /runtime: binary 1\.4\.0 \(linux-x86_64\)/);
    assert.match(res.stdout, /TANDEM-BINARY-RAN/);
  });
});

describe('run.sh --fresh', () => {
  function setupCoord(env) {
    const coord = join(env.pkg, '.agent-workspace', 'coordination');
    for (const sub of ['claims', 'conversations', 'queries', 'sessions']) {
      mkdirSync(join(coord, sub), { recursive: true });
    }
    writeFileSync(join(coord, '.gitignore'), '*\n!.gitignore\n');
    writeFileSync(join(coord, '.DS_Store'), 'junk');
    writeFileSync(join(coord, 'extra.txt'), 'extra');
    writeFileSync(join(coord, 'claims', 'c-1.json'), '{"task":"t-1"}');
    writeFileSync(join(coord, 'conversations', 'conv.log'), 'log data');
    writeFileSync(join(coord, 'queries', 'q-1.json'), '{"q":"help?"}');
    writeFileSync(join(coord, 'sessions', 's-1.json'), '{"sess":"impl"}');
    return coord;
  }

  it('removes files under coordination dir and keeps subdirs plus .gitignore', async () => {
    const env = setup();
    const coord = setupCoord(env);
    const r = await run(env, ['--fresh', '--yes']);
    assert.equal(r.code, 0, r.out);
    assert.match(r.out, /Cleared 6 files/);
    assert.ok(existsSync(join(coord, '.gitignore')), '.gitignore must be kept');
    assert.ok(!existsSync(join(coord, '.DS_Store')), '.DS_Store must be deleted');
    assert.ok(!existsSync(join(coord, 'extra.txt')), 'extra.txt must be deleted');
    assert.ok(!existsSync(join(coord, 'claims', 'c-1.json')), 'claim file deleted');
    assert.ok(!existsSync(join(coord, 'conversations', 'conv.log')), 'conversation deleted');
    assert.ok(!existsSync(join(coord, 'queries', 'q-1.json')), 'query deleted');
    assert.ok(!existsSync(join(coord, 'sessions', 's-1.json')), 'session deleted');
    for (const sub of ['claims', 'conversations', 'queries', 'sessions']) {
      assert.ok(existsSync(join(coord, sub)), `subdir ${sub} must be kept`);
    }
    assert.match(r.stdout, /TANDEM-BINARY-RAN args=--fresh/);
  });

  it('forwards --fresh to tandem exactly once even if passed in options', async () => {
    const env = setup();
    setupCoord(env);
    const r = await run(env, ['--fresh', '--yes', '--', '--fresh']);
    assert.equal(r.code, 0, r.out);
    assert.match(r.stdout, /TANDEM-BINARY-RAN args=--fresh\n/);
  });

  it('declined with --no exits 2 and deletes nothing', async () => {
    const env = setup();
    const coord = setupCoord(env);
    const r = await run(env, ['--fresh', '--no']);
    assert.equal(r.code, 2, r.out);
    assert.ok(existsSync(join(coord, 'claims', 'c-1.json')), 'file still exists');
  });

  it('declined with --answers n exits 2 and deletes nothing', async () => {
    const env = setup();
    const coord = setupCoord(env);
    const r = await run(env, ['--fresh', '--answers', 'n']);
    assert.equal(r.code, 2, r.out);
    assert.ok(existsSync(join(coord, 'claims', 'c-1.json')), 'file still exists');
  });

  it('non-interactive without --yes exits 6 and deletes nothing', async () => {
    const env = setup();
    const coord = setupCoord(env);
    const r = await run(env, ['--fresh']);
    assert.equal(r.code, 6, r.out);
    assert.ok(existsSync(join(coord, 'claims', 'c-1.json')), 'file still exists');
  });

  it('--check reports would delete and does not modify files', async () => {
    const env = setup();
    const coord = setupCoord(env);
    const r = await run(env, ['--fresh', '--check']);
    assert.match(r.out, /Would delete 6 files in \.agent-workspace\/coordination/);
    assert.ok(existsSync(join(coord, 'claims', 'c-1.json')), 'file untouched');
  });

  it('--check creates nothing when the coordination dir is missing', async () => {
    const env = setup();
    const coord = join(env.pkg, '.agent-workspace', 'coordination');
    const r = await run(env, ['--fresh', '--check']);
    assert.match(r.out, /Nothing to clear/);
    assert.ok(!existsSync(coord), 'coordination dir must not be created by --check');
  });

  it('refuses to clear when tandem is running and exits 2', async () => {
    const env = setup();
    const coord = setupCoord(env);
    env.mark('tmux.running');
    const r = await run(env, ['--fresh', '--yes']);
    assert.equal(r.code, 2, r.out);
    assert.match(r.out, /tandem is running for this workspace/);
    assert.match(r.out, /Quit it first/);
    assert.ok(existsSync(join(coord, 'claims', 'c-1.json')), 'file untouched');
  });

  it('handles empty coordination directory (nothing to clear)', async () => {
    const env = setup();
    const coord = join(env.pkg, '.agent-workspace', 'coordination');
    mkdirSync(join(coord, 'claims'), { recursive: true });
    writeFileSync(join(coord, '.gitignore'), '*\n!.gitignore\n');
    const r = await run(env, ['--fresh', '--yes']);
    assert.equal(r.code, 0, r.out);
    assert.match(r.out, /Nothing to clear/);
  });

  it('creates missing coordination directory and reports nothing to clear', async () => {
    const env = setup();
    const coord = join(env.pkg, '.agent-workspace', 'coordination');
    if (existsSync(coord)) rmSync(coord, { recursive: true, force: true });
    const r = await run(env, ['--fresh', '--yes']);
    assert.equal(r.code, 0, r.out);
    assert.match(r.out, /Nothing to clear/);
    assert.ok(existsSync(coord), 'coordination directory created');
  });

  it('rejects symlinked coordination directory with exit 4', async () => {
    const env = setup();
    const outside = join(env.base, 'outside-coord');
    mkdirSync(outside, { recursive: true });
    const coord = join(env.pkg, '.agent-workspace', 'coordination');
    mkdirSync(dirname(coord), { recursive: true });
    if (existsSync(coord)) rmSync(coord, { recursive: true, force: true });
    symlinkSync(outside, coord);
    const r = await run(env, ['--fresh', '--yes']);
    assert.equal(r.code, 4, r.out);
  });

  it('honors TANDEM_COORD_DIR', async () => {
    const env = setup();
    const custom = join(env.base, 'custom-coord');
    for (const sub of ['claims', 'conversations', 'queries', 'sessions']) {
      mkdirSync(join(custom, sub), { recursive: true });
    }
    writeFileSync(join(custom, '.gitignore'), '*\n');
    writeFileSync(join(custom, 'claims', 'custom.json'), '{}');
    const r = await run(env, ['--fresh', '--yes'], { extraEnv: { TANDEM_COORD_DIR: custom } });
    assert.equal(r.code, 0, r.out);
    assert.ok(!existsSync(join(custom, 'claims', 'custom.json')));
    assert.ok(existsSync(join(custom, '.gitignore')));
  });

  it('--help documents --fresh', async () => {
    const env = setup();
    const r = await run(env, ['--help']);
    assert.equal(r.code, 0);
    assert.match(r.out, /--fresh/);
  });
});
