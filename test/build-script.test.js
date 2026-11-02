import { describe, it, afterEach } from 'node:test';
import assert from 'node:assert/strict';
import { spawnSync, execFileSync } from 'node:child_process';
import { chmodSync, copyFileSync, existsSync, mkdirSync, readFileSync, readdirSync, rmSync, writeFileSync } from 'node:fs';
import { join, dirname } from 'node:path';
import { fileURLToPath } from 'node:url';
import { makeTmp, cleanup } from './helpers.js';

const ROOT = dirname(dirname(fileURLToPath(import.meta.url)));
const BUILD_SH = join(ROOT, 'tandem', 'packaging', 'build.sh');
const VERIFY_SH = join(ROOT, 'tandem', 'packaging', 'verify-versions.sh');
const VERSION = '1.5.0';
const SYSTEM_PATH = '/usr/bin:/bin';

const dirs = [];
afterEach(() => {
  while (dirs.length) cleanup(dirs.pop());
});

const CARGO_STUB = `#!/bin/sh
echo "$*" >> "$STUB_DIR/cargo.calls"
echo "MACOSX_DEPLOYMENT_TARGET=\${MACOSX_DEPLOYMENT_TARGET-unset} RUSTFLAGS=\${RUSTFLAGS-unset}" >> "$STUB_DIR/cargo.env"
[ -f "$STUB_DIR/cargo.fail" ] && exit 1
[ -f "$STUB_DIR/cargo.nobinary" ] && exit 0
target=""
while [ $# -gt 0 ]; do
  [ "$1" = "--target" ] && target="$2"
  shift
done
case "$target" in *windows*) exe=tandem.exe;; *) exe=tandem;; esac
outdir="${CARGO_TARGET_DIR:-$CRATE_ROOT/target}/$target/release"
mkdir -p "$outdir"
ver="\${STUB_BINARY_VERSION:-${VERSION}}"
printf '#!/bin/sh\\ncase "$1" in --version) echo "tandem %s";; *) echo "tandem help";; esac\\n' "$ver" > "$outdir/$exe"
chmod +x "$outdir/$exe"
`;

function setup() {
  const tmp = makeTmp();
  dirs.push(tmp);
  const crate = join(tmp, 'tandem');
  const pkgDir = join(crate, 'packaging');
  mkdirSync(pkgDir, { recursive: true });
  copyFileSync(BUILD_SH, join(pkgDir, 'build.sh'));
  chmodSync(join(pkgDir, 'build.sh'), 0o755);
  writeFileSync(join(crate, 'Cargo.toml'), `[package]\nname = "tandem"\nversion = "${VERSION}"\nedition = "2024"\n`);
  const stub = join(tmp, 'stub');
  mkdirSync(stub, { recursive: true });
  writeFileSync(join(stub, 'cargo'), CARGO_STUB);
  chmodSync(join(stub, 'cargo'), 0o755);
  return { tmp, crate, stub, script: join(pkgDir, 'build.sh'), out: join(crate, 'dist') };
}

function build(env, args = [], extraEnv = {}) {
  const res = spawnSync('bash', [env.script, ...args], {
    cwd: env.tmp,
    env: { PATH: `${env.stub}:${SYSTEM_PATH}`, HOME: env.tmp, STUB_DIR: env.stub, CRATE_ROOT: env.crate, ...extraEnv },
    encoding: 'utf8',
  });
  return { code: res.status, stdout: res.stdout, stderr: res.stderr };
}

function lib(fnCall) {
  const res = spawnSync('bash', ['-c', `BUILD_SH_SOURCE_ONLY=1 . "${BUILD_SH}"; ${fnCall}`], { encoding: 'utf8' });
  return { code: res.status, stdout: res.stdout, stderr: res.stderr };
}

function listTar(path) {
  return execFileSync('tar', ['-tzf', path]).toString().split('\n').filter(Boolean);
}

function listZip(path) {
  return execFileSync('unzip', ['-Z1', path]).toString().split('\n').filter(Boolean);
}

function sha256(path) {
  try {
    return execFileSync('shasum', ['-a', '256', path]).toString().split(' ')[0];
  } catch {
    return execFileSync('sha256sum', [path]).toString().split(' ')[0];
  }
}

describe('build.sh platform slug', () => {
  const cases = [
    ['aarch64-apple-darwin', 'darwin-arm64'],
    ['x86_64-apple-darwin', 'darwin-x86_64'],
    ['x86_64-unknown-linux-musl', 'linux-x86_64'],
    ['aarch64-unknown-linux-musl', 'linux-arm64'],
    ['x86_64-pc-windows-msvc', 'windows-x86_64'],
  ];
  for (const [triple, slug] of cases) {
    it(`maps ${triple} to ${slug}`, () => {
      const res = lib(`slug_for_triple ${triple}`);
      assert.equal(res.code, 0);
      assert.equal(res.stdout, slug);
    });
  }

  it('rejects a triple outside the supported matrix', () => {
    const res = lib('slug_for_triple x86_64-unknown-linux-gnu');
    assert.notEqual(res.code, 0);
  });

  it('builds asset names matching the run.sh download contract', () => {
    assert.equal(lib('asset_name 1.5.0 darwin-arm64').stdout, 'tandem-1.5.0-darwin-arm64.tar.gz');
    assert.equal(lib('asset_name 1.5.0 linux-x86_64').stdout, 'tandem-1.5.0-linux-x86_64.tar.gz');
    assert.equal(lib('asset_name 1.5.0 windows-x86_64').stdout, 'tandem-1.5.0-windows-x86_64.zip');
  });
});

describe('build.sh cargo invocation', () => {
  it('runs a locked release build for the requested target', () => {
    const env = setup();
    const res = build(env, ['--target', 'aarch64-apple-darwin']);
    assert.equal(res.code, 0, res.stderr);
    const calls = readFileSync(join(env.stub, 'cargo.calls'), 'utf8').trim().split('\n');
    assert.deepEqual(calls, ['build --release --locked --target aarch64-apple-darwin']);
  });

  it('pins the macOS 13.0 floor through target-scoped RUSTFLAGS for apple triples only', () => {
    const apple = setup();
    build(apple, ['--target', 'x86_64-apple-darwin']);
    const appleEnv = readFileSync(join(apple.stub, 'cargo.env'), 'utf8');
    assert.match(appleEnv, /RUSTFLAGS=.*-C link-arg=-mmacosx-version-min=13\.0/);
    assert.match(appleEnv, /MACOSX_DEPLOYMENT_TARGET=unset/);

    const linux = setup();
    build(linux, ['--target', 'x86_64-unknown-linux-musl']);
    assert.match(readFileSync(join(linux.stub, 'cargo.env'), 'utf8'), /RUSTFLAGS=unset/);
  });

  it('keeps caller RUSTFLAGS when adding the macOS floor', () => {
    const env = setup();
    build(env, ['--target', 'aarch64-apple-darwin'], { RUSTFLAGS: '-D warnings' });
    assert.match(readFileSync(join(env.stub, 'cargo.env'), 'utf8'), /RUSTFLAGS=-D warnings -C link-arg=-mmacosx-version-min=13\.0/);
  });
});

describe('build.sh target directory', () => {
  it('resolves a relative CARGO_TARGET_DIR from the caller, not the crate folder', () => {
    const env = setup();
    const callerDir = join(env.tmp, 'caller');
    mkdirSync(callerDir, { recursive: true });
    const res = spawnSync('bash', [env.script, '--target', 'aarch64-apple-darwin'], {
      cwd: callerDir,
      env: { PATH: `${env.stub}:${SYSTEM_PATH}`, HOME: env.tmp, STUB_DIR: env.stub, CRATE_ROOT: env.crate, CARGO_TARGET_DIR: 'out-target' },
      encoding: 'utf8',
    });
    assert.equal(res.status, 0, res.stderr);
    assert.ok(existsSync(join(env.out, `tandem-${VERSION}-darwin-arm64.tar.gz`)));
  });
});

describe('build.sh archives', () => {
  it('writes a tar.gz with a top-level tandem/ folder holding the binary', () => {
    const env = setup();
    const res = build(env, ['--target', 'aarch64-apple-darwin']);
    assert.equal(res.code, 0, res.stderr);
    const archive = join(env.out, `tandem-${VERSION}-darwin-arm64.tar.gz`);
    assert.equal(res.stdout.trim(), archive);
    assert.ok(existsSync(archive));
    const names = listTar(archive).map(n => n.replace(/\/$/, ''));
    assert.ok(names.includes('tandem/tandem'));
    assert.ok(names.every(n => n === 'tandem' || n.startsWith('tandem/')));
  });

  it('writes a zip with tandem/tandem.exe for the windows target', () => {
    const env = setup();
    const res = build(env, ['--target', 'x86_64-pc-windows-msvc']);
    assert.equal(res.code, 0, res.stderr);
    const archive = join(env.out, `tandem-${VERSION}-windows-x86_64.zip`);
    assert.ok(existsSync(archive));
    const names = listZip(archive).map(n => n.replace(/\/$/, ''));
    assert.ok(names.includes('tandem/tandem.exe'));
    assert.ok(names.every(n => n === 'tandem' || n.startsWith('tandem/')));
  });

  it('writes a <hex>  <name> checksum file next to the archive', () => {
    const env = setup();
    build(env, ['--target', 'x86_64-unknown-linux-musl']);
    const archive = join(env.out, `tandem-${VERSION}-linux-x86_64.tar.gz`);
    const shaFile = `${archive}.sha256`;
    assert.ok(existsSync(shaFile));
    assert.equal(readFileSync(shaFile, 'utf8'), `${sha256(archive)}  tandem-${VERSION}-linux-x86_64.tar.gz\n`);
  });

  it('leaves only the archive and checksum plus the staged folder in dist', () => {
    const env = setup();
    build(env, ['--target', 'aarch64-unknown-linux-musl']);
    assert.deepEqual(readdirSync(env.out).sort(), [
      'tandem',
      `tandem-${VERSION}-linux-arm64.tar.gz`,
      `tandem-${VERSION}-linux-arm64.tar.gz.sha256`,
    ]);
  });
});

describe('build.sh failure modes', () => {
  it('fails loudly on an unsupported target', () => {
    const env = setup();
    const res = build(env, ['--target', 'x86_64-unknown-linux-gnu']);
    assert.notEqual(res.code, 0);
    assert.match(res.stderr, /unsupported target/i);
    assert.ok(!existsSync(join(env.stub, 'cargo.calls')));
  });

  it('fails when cargo fails', () => {
    const env = setup();
    writeFileSync(join(env.stub, 'cargo.fail'), '1');
    const res = build(env, ['--target', 'aarch64-apple-darwin']);
    assert.notEqual(res.code, 0);
    assert.ok(!existsSync(env.out) || readdirSync(env.out).every(n => !n.endsWith('.tar.gz')));
  });

  it('fails when cargo leaves no binary for the target', () => {
    const env = setup();
    writeFileSync(join(env.stub, 'cargo.nobinary'), '1');
    const res = build(env, ['--target', 'aarch64-apple-darwin']);
    assert.notEqual(res.code, 0);
    assert.match(res.stderr, /built binary not found/i);
  });

  it('fails when --expect-version differs from Cargo.toml', () => {
    const env = setup();
    const res = build(env, ['--target', 'aarch64-apple-darwin', '--expect-version', '9.9.9']);
    assert.notEqual(res.code, 0);
    assert.match(res.stderr, /version mismatch/i);
    assert.ok(!existsSync(join(env.stub, 'cargo.calls')));
  });

  it('fails when a host-runnable binary reports a different version', () => {
    const env = setup();
    const hostTriple = hostTripleFor(process.platform, process.arch);
    if (!hostTriple) return;
    const res = build(env, ['--target', hostTriple], { STUB_BINARY_VERSION: '0.0.1' });
    assert.notEqual(res.code, 0);
    assert.match(res.stderr, /unexpected version output/i);
  });

  it('smoke-tests a host-runnable binary and passes when versions agree', () => {
    const env = setup();
    const hostTriple = hostTripleFor(process.platform, process.arch);
    if (!hostTriple) return;
    const res = build(env, ['--target', hostTriple]);
    assert.equal(res.code, 0, res.stderr);
  });
});

function hostTripleFor(platform, arch) {
  if (platform === 'darwin' && arch === 'arm64') return 'aarch64-apple-darwin';
  if (platform === 'darwin' && arch === 'x64') return 'x86_64-apple-darwin';
  if (platform === 'linux' && arch === 'x64') return 'x86_64-unknown-linux-musl';
  if (platform === 'linux' && arch === 'arm64') return 'aarch64-unknown-linux-musl';
  return null;
}

describe('verify-versions.sh', () => {
  function repo(pkgVersion, cargoVersion) {
    const tmp = makeTmp();
    dirs.push(tmp);
    mkdirSync(join(tmp, 'tandem', 'packaging'), { recursive: true });
    copyFileSync(VERIFY_SH, join(tmp, 'tandem', 'packaging', 'verify-versions.sh'));
    writeFileSync(join(tmp, 'package.json'), JSON.stringify({ name: 'agent-workspace', version: pkgVersion }, null, 2));
    writeFileSync(join(tmp, 'tandem', 'Cargo.toml'), `[package]\nname = "tandem"\nversion = "${cargoVersion}"\n\n[dependencies]\nserde = { version = "1.0" }\n`);
    return tmp;
  }

  function verify(tmp, ...args) {
    const res = spawnSync('bash', [join(tmp, 'tandem', 'packaging', 'verify-versions.sh'), ...args], {
      cwd: tmp,
      env: { PATH: SYSTEM_PATH },
      encoding: 'utf8',
    });
    return { code: res.status, stdout: res.stdout, stderr: res.stderr };
  }

  it('passes when tag, package.json and Cargo.toml agree', () => {
    const res = verify(repo('1.6.0', '1.6.0'), 'v1.6.0');
    assert.equal(res.code, 0, res.stderr);
    assert.match(res.stdout, /1\.6\.0/);
  });

  it('accepts a tag without the v prefix', () => {
    assert.equal(verify(repo('1.6.0', '1.6.0'), '1.6.0').code, 0);
  });

  it('fails when package.json differs from the tag', () => {
    const res = verify(repo('1.5.0', '1.6.0'), 'v1.6.0');
    assert.notEqual(res.code, 0);
    assert.match(res.stderr, /package\.json version '1\.5\.0' != tag version '1\.6\.0'/);
  });

  it('fails when Cargo.toml differs from the tag', () => {
    const res = verify(repo('1.6.0', '1.5.0'), 'v1.6.0');
    assert.notEqual(res.code, 0);
    assert.match(res.stderr, /Cargo\.toml version '1\.5\.0' != tag version '1\.6\.0'/);
  });

  it('ignores dependency versions in Cargo.toml', () => {
    assert.equal(verify(repo('1.6.0', '1.6.0'), 'v1.6.0').code, 0);
  });

  it('fails without a tag argument', () => {
    const res = verify(repo('1.6.0', '1.6.0'));
    assert.notEqual(res.code, 0);
    assert.match(res.stderr, /usage/i);
  });

  it('fails when Cargo.toml is missing', () => {
    const tmp = repo('1.6.0', '1.6.0');
    rmSync(join(tmp, 'tandem', 'Cargo.toml'));
    const res = verify(tmp, 'v1.6.0');
    assert.notEqual(res.code, 0);
    assert.match(res.stderr, /Cargo\.toml/);
  });
});
