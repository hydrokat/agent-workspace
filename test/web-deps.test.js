import { describe, it } from 'node:test';
import assert from 'node:assert/strict';
import { chmodSync, mkdirSync, mkdtempSync, readFileSync, statSync, writeFileSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
import {
  checkWebDeps,
  detectHostProblem,
  detectPlatform,
  ensureWebDeps,
  findExecutable,
  planFromFacts,
} from '../src/commands/web-deps.js';

const MAC_ARM = { os: 'macos', arch: 'arm64', libc: null };
const LINUX_X64 = { os: 'linux', arch: 'x64', libc: 'glibc' };

function facts(over = {}) {
  return {
    platform: MAC_ARM,
    bun: 'missing',
    ttyd: false,
    needTerminal: false,
    hostProblem: null,
    tools: ['curl', 'unzip'],
    ...over,
  };
}

describe('detectPlatform', () => {
  it('maps node platform and arch to bun terms', () => {
    assert.deepEqual(detectPlatform({ platform: 'darwin', arch: 'arm64' }), MAC_ARM);
    assert.deepEqual(detectPlatform({ platform: 'darwin', arch: 'x64' }), { os: 'macos', arch: 'x64', libc: null });
    assert.deepEqual(detectPlatform({ platform: 'win32', arch: 'x64' }), { os: 'windows', arch: 'x64', libc: null });
  });

  it('detects glibc vs musl on Linux from the dynamic loader', () => {
    assert.equal(detectPlatform({ platform: 'linux', arch: 'x64', exists: () => false }).libc, 'glibc');
    const musl = detectPlatform({ platform: 'linux', arch: 'arm64', exists: (p) => p === '/lib/ld-musl-aarch64.so.1' });
    assert.deepEqual(musl, { os: 'linux', arch: 'arm64', libc: 'musl' });
    assert.equal(detectPlatform({ platform: 'linux', arch: 'x64', exists: (p) => p === '/lib/ld-musl-x86_64.so.1' }).libc, 'musl');
  });

  it('passes unknown platforms through so they can be reported as unsupported', () => {
    assert.deepEqual(detectPlatform({ platform: 'freebsd', arch: 'riscv64' }), { os: 'freebsd', arch: 'riscv64', libc: null });
  });
});

describe('detectHostProblem', () => {
  it('flags macOS older than 13', () => {
    const run = () => ({ ok: true, stdout: '12.7.4\n' });
    assert.match(detectHostProblem({ platform: MAC_ARM, run }), /macOS 12\.7\.4.*13/);
  });

  it('accepts macOS 13 and later and unknown versions', () => {
    assert.equal(detectHostProblem({ platform: MAC_ARM, run: () => ({ ok: true, stdout: '14.5\n' }) }), null);
    assert.equal(detectHostProblem({ platform: MAC_ARM, run: () => ({ ok: false, stdout: '' }) }), null);
  });

  it('flags a Linux x64 CPU without SSE4.2 and accepts one with it', () => {
    const without = () => 'flags : fpu sse2 ssse3';
    const withSse = () => 'flags : fpu sse2 sse4_2 avx2';
    assert.match(detectHostProblem({ platform: LINUX_X64, readFile: without }), /SSE4\.2/);
    assert.equal(detectHostProblem({ platform: LINUX_X64, readFile: withSse }), null);
  });

  it('does not check CPU flags on arm64 or when cpuinfo is unreadable', () => {
    assert.equal(detectHostProblem({ platform: { os: 'linux', arch: 'arm64', libc: 'glibc' }, readFile: () => 'nothing' }), null);
    assert.equal(detectHostProblem({ platform: LINUX_X64, readFile: () => { throw new Error('nope'); } }), null);
  });
});

describe('planFromFacts', () => {
  it('has nothing to do when bun runs and no terminal is requested', () => {
    const plan = planFromFacts(facts({ bun: 'ok' }));
    assert.equal(plan.bunMissing, false);
    assert.deepEqual(plan.autoInstall, []);
    assert.deepEqual(plan.manual, []);
    assert.equal(plan.script, null);
    assert.equal(plan.canAutoInstall, false);
  });

  it('offers to install bun with the official installer on macOS', () => {
    const plan = planFromFacts(facts());
    assert.equal(plan.supported, true);
    assert.deepEqual(plan.autoInstall, ['bun']);
    assert.equal(plan.script.name, 'install-web-deps.sh');
    assert.match(plan.script.content, /^#!\/usr\/bin\/env bash\n/);
    assert.match(plan.script.content, /for macOS arm64/);
    assert.match(plan.script.content, /https:\/\/bun\.com\/docs\/installation/);
    assert.match(plan.script.content, /set -euo pipefail/);
    assert.match(plan.script.content, /curl -fsSL https:\/\/bun\.com\/install \| bash/);
    assert.match(plan.script.content, /export BUN_INSTALL="\$\{BUN_INSTALL:-\$HOME\/\.bun\}"/);
    assert.match(plan.script.content, /export PATH="\$BUN_INSTALL\/bin:\$PATH"/);
    assert.match(plan.script.content, /\nbun --version\n/);
    assert.doesNotMatch(plan.script.content, /ttyd/);
  });

  it('uses the official bun.com URL, not the legacy bun.sh one, for macOS and Linux', () => {
    for (const platform of [MAC_ARM, LINUX_X64]) {
      const { script } = planFromFacts(facts({ platform }));
      assert.doesNotMatch(script.content, /bun\.sh\/install(?!\.ps1)/);
    }
  });

  it('labels the Linux script with arch and libc', () => {
    assert.match(planFromFacts(facts({ platform: LINUX_X64 })).script.content, /for Linux x64 \(glibc\)/);
    const musl = { os: 'linux', arch: 'arm64', libc: 'musl' };
    assert.match(planFromFacts(facts({ platform: musl })).script.content, /for Linux arm64 \(musl\)/);
  });

  it('never installs with elevated privileges', () => {
    const cases = [
      facts(),
      facts({ platform: LINUX_X64 }),
      facts({ bun: 'ok', needTerminal: true, tools: ['curl', 'unzip', 'brew'] }),
      facts({ platform: { os: 'windows', arch: 'x64', libc: null } }),
    ];
    for (const c of cases) {
      const plan = planFromFacts(c);
      if (plan.script) assert.doesNotMatch(plan.script.content, /sudo|runas|Start-Process -Verb/i);
    }
  });

  it('refuses to auto-install on Linux when unzip is missing and tells the user the distro command', () => {
    const apt = planFromFacts(facts({ platform: LINUX_X64, tools: ['curl', 'apt'] }));
    assert.equal(apt.canAutoInstall, false);
    assert.equal(apt.script, null);
    assert.deepEqual(apt.manual[0], { dep: 'unzip', commands: ['sudo apt install unzip'] });
    assert.equal(apt.manual[1].dep, 'bun');

    const dnf = planFromFacts(facts({ platform: LINUX_X64, tools: ['curl', 'dnf'] }));
    assert.deepEqual(dnf.manual[0].commands, ['sudo dnf install unzip']);
    const apk = planFromFacts(facts({ platform: LINUX_X64, tools: ['curl', 'apk'] }));
    assert.deepEqual(apk.manual[0].commands, ['apk add unzip']);
  });

  it('falls back to the documented apt command when no package manager is recognised', () => {
    const plan = planFromFacts(facts({ platform: LINUX_X64, tools: ['curl'] }));
    assert.deepEqual(plan.manual[0].commands, ['sudo apt install unzip']);
  });

  it('requires curl on macOS and Linux', () => {
    const plan = planFromFacts(facts({ platform: LINUX_X64, tools: ['unzip', 'apt'] }));
    assert.equal(plan.canAutoInstall, false);
    assert.equal(plan.manual[0].dep, 'curl');
    assert.deepEqual(plan.manual[0].commands, ['sudo apt install curl']);
  });

  it('does not need unzip on macOS', () => {
    const plan = planFromFacts(facts({ tools: ['curl'] }));
    assert.deepEqual(plan.autoInstall, ['bun']);
  });

  it('uses the PowerShell installer on Windows', () => {
    const plan = planFromFacts(facts({ platform: { os: 'windows', arch: 'x64', libc: null }, tools: [] }));
    assert.deepEqual(plan.autoInstall, ['bun']);
    assert.equal(plan.script.name, 'install-web-deps.ps1');
    assert.match(plan.script.content, /for Windows x64/);
    assert.match(plan.script.content, /\$ErrorActionPreference = 'Stop'/);
    assert.match(plan.script.content, /powershell -c "irm bun\.sh\/install\.ps1\|iex"/);
    assert.match(plan.script.content, /\$env:USERPROFILE\\\.bun\\bin\\bun" --version/);
  });

  it('reports platforms bun does not ship for as unsupported, with a pointer to the docs', () => {
    const plan = planFromFacts(facts({ platform: { os: 'freebsd', arch: 'riscv64', libc: null } }));
    assert.equal(plan.supported, false);
    assert.match(plan.unsupportedReason, /freebsd riscv64/);
    assert.equal(plan.canAutoInstall, false);
    assert.equal(plan.script, null);
    assert.match(plan.manual[0].commands[0], /https:\/\/bun\.com\/docs\/installation/);
  });

  it('reports a host problem (old macOS, CPU without SSE4.2) as unsupported', () => {
    const plan = planFromFacts(facts({ hostProblem: 'macOS 12.7 is older than 13.0' }));
    assert.equal(plan.supported, false);
    assert.equal(plan.unsupportedReason, 'macOS 12.7 is older than 13.0');
    assert.equal(plan.script, null);
  });

  it('ignores host problems when bun already runs', () => {
    const plan = planFromFacts(facts({ bun: 'ok', hostProblem: 'macOS 12.7 is older than 13.0' }));
    assert.equal(plan.bunMissing, false);
    assert.equal(plan.supported, true);
  });

  it('treats a bun binary that is present but does not run as missing and explains why', () => {
    const plan = planFromFacts(facts({ bun: 'broken' }));
    assert.equal(plan.bunMissing, true);
    assert.equal(plan.bunBroken, true);
    assert.deepEqual(plan.autoInstall, ['bun']);
  });

  it('lists manual commands for bun, including Homebrew or Scoop when available', () => {
    const mac = planFromFacts(facts({ tools: ['curl', 'brew'] }));
    assert.deepEqual(mac.manual.find((m) => m.dep === 'bun').commands, [
      'curl -fsSL https://bun.com/install | bash',
      'brew install oven-sh/bun/bun',
    ]);
    const win = planFromFacts(facts({ platform: { os: 'windows', arch: 'x64', libc: null }, tools: ['scoop'] }));
    assert.deepEqual(win.manual.find((m) => m.dep === 'bun').commands, [
      'powershell -c "irm bun.sh/install.ps1|iex"',
      'scoop install bun',
    ]);
  });

  describe('ttyd (only when the terminal feature is on)', () => {
    it('is ignored when the terminal is not requested', () => {
      const plan = planFromFacts(facts({ bun: 'ok', ttyd: false, needTerminal: false }));
      assert.equal(plan.ttydMissing, false);
      assert.deepEqual(plan.manual, []);
    });

    it('is installed with Homebrew on macOS when brew exists, after bun', () => {
      const plan = planFromFacts(facts({ needTerminal: true, tools: ['curl', 'brew'] }));
      assert.deepEqual(plan.autoInstall, ['bun', 'ttyd']);
      assert.match(plan.script.content, /echo "==> Installing ttyd"\nbrew install ttyd\n/);
      assert.ok(plan.script.content.indexOf('Installing bun') < plan.script.content.indexOf('Installing ttyd'));
    });

    it('produces a ttyd-only script when bun already runs', () => {
      const plan = planFromFacts(facts({ bun: 'ok', needTerminal: true, tools: ['brew'] }));
      assert.deepEqual(plan.autoInstall, ['ttyd']);
      assert.doesNotMatch(plan.script.content, /Installing bun|bun\.com\/install/);
    });

    it('is manual on Linux (needs a system package manager) and without Homebrew on macOS', () => {
      const linux = planFromFacts(facts({ platform: LINUX_X64, bun: 'ok', needTerminal: true, tools: ['apt'] }));
      assert.deepEqual(linux.autoInstall, []);
      assert.deepEqual(linux.manual, [{ dep: 'ttyd', commands: ['sudo apt install ttyd'] }]);
      const mac = planFromFacts(facts({ bun: 'ok', needTerminal: true, tools: [] }));
      assert.match(mac.manual[0].commands[0], /https:\/\/brew\.sh.*brew install ttyd/);
    });
  });
});

describe('findExecutable and checkWebDeps', () => {
  function sandbox() {
    const dir = mkdtempSync(join(tmpdir(), 'wd-'));
    const bin = join(dir, 'bin');
    const home = join(dir, 'home');
    mkdirSync(bin, { recursive: true });
    mkdirSync(join(home, '.bun', 'bin'), { recursive: true });
    return { dir, bin, home };
  }
  function exe(path) {
    writeFileSync(path, '#!/bin/sh\n');
    chmodSync(path, 0o755);
  }

  it('finds an executable on PATH', () => {
    const s = sandbox();
    exe(join(s.bin, 'curl'));
    assert.equal(findExecutable('curl', { env: { PATH: s.bin, HOME: s.home } }), join(s.bin, 'curl'));
    assert.equal(findExecutable('nope', { env: { PATH: s.bin, HOME: s.home } }), null);
  });

  it('finds bun in ~/.bun/bin even when it is not on PATH', () => {
    const s = sandbox();
    exe(join(s.home, '.bun', 'bin', 'bun'));
    const env = { PATH: s.bin, HOME: s.home };
    assert.equal(findExecutable('bun', { env, extraDirs: [join(s.home, '.bun', 'bin')] }), join(s.home, '.bun', 'bin', 'bun'));
  });

  it('honours BUN_INSTALL', () => {
    const s = sandbox();
    mkdirSync(join(s.dir, 'custom', 'bin'), { recursive: true });
    exe(join(s.dir, 'custom', 'bin', 'bun'));
    const r = checkWebDeps({
      env: { PATH: s.bin, HOME: s.home, BUN_INSTALL: join(s.dir, 'custom') },
      platform: LINUX_X64,
      run: () => ({ ok: true, stdout: '1.3.3\n' }),
      readFile: () => 'sse4_2',
    });
    assert.equal(r.facts.bun, 'ok');
    assert.equal(r.bunPath, join(s.dir, 'custom', 'bin', 'bun'));
  });

  it('reports bun as ok with its version when it runs', () => {
    const s = sandbox();
    exe(join(s.bin, 'bun'));
    const r = checkWebDeps({ env: { PATH: s.bin, HOME: s.home }, platform: MAC_ARM, run: () => ({ ok: true, stdout: '1.3.3\n' }) });
    assert.equal(r.facts.bun, 'ok');
    assert.equal(r.bunVersion, '1.3.3');
  });

  it('reports bun as broken when the binary exists but does not run on this host', () => {
    const s = sandbox();
    exe(join(s.bin, 'bun'));
    const r = checkWebDeps({ env: { PATH: s.bin, HOME: s.home }, platform: MAC_ARM, run: () => ({ ok: false, stdout: '' }) });
    assert.equal(r.facts.bun, 'broken');
  });

  it('reports bun as missing when it cannot be found', () => {
    const s = sandbox();
    const r = checkWebDeps({ env: { PATH: s.bin, HOME: s.home }, platform: MAC_ARM, run: () => ({ ok: true, stdout: '' }) });
    assert.equal(r.facts.bun, 'missing');
  });

  it('checks ttyd only when the terminal feature is requested, and records available tools', () => {
    const s = sandbox();
    for (const t of ['curl', 'unzip', 'brew', 'apt']) exe(join(s.bin, t));
    const env = { PATH: s.bin, HOME: s.home };
    const off = checkWebDeps({ env, platform: LINUX_X64, run: () => ({ ok: true, stdout: '' }), readFile: () => 'sse4_2' });
    assert.equal(off.facts.needTerminal, false);
    assert.deepEqual([...off.facts.tools].sort(), ['apt', 'brew', 'curl', 'unzip']);
    exe(join(s.bin, 'ttyd'));
    const on = checkWebDeps({ env, platform: LINUX_X64, needTerminal: true, run: () => ({ ok: true, stdout: '' }), readFile: () => 'sse4_2' });
    assert.equal(on.facts.ttyd, true);
  });
});

describe('ensureWebDeps (ask, then install or explain)', () => {
  const READY = { facts: facts({ bun: 'ok' }), bunPath: '/usr/local/bin/bun', bunVersion: '1.3.3' };
  const MISSING = { facts: facts(), bunPath: null, bunVersion: null };

  function harness({ checks, answer = 'install', isTty = true, yes = false, noInstall = false, runStatus = 0 }) {
    const out = [];
    const calls = { asked: [], ran: [], wrote: [] };
    let i = 0;
    const stateDir = mkdtempSync(join(tmpdir(), 'wd-state-'));
    return {
      out,
      calls,
      stateDir,
      opts: {
        stateDir,
        yes,
        noInstall,
        isTty,
        check: () => checks[Math.min(i++, checks.length - 1)],
        ask: async (q) => { calls.asked.push(q); return answer; },
        runScript: (cmd, args) => { calls.ran.push([cmd, ...args]); return { status: runStatus }; },
        print: (line) => out.push(line),
      },
    };
  }

  it('does nothing and asks nothing when bun is already installed', async () => {
    const h = harness({ checks: [READY] });
    const r = await ensureWebDeps(h.opts);
    assert.equal(r.action, 'present');
    assert.equal(r.ok, true);
    assert.equal(r.bunPath, '/usr/local/bin/bun');
    assert.deepEqual(h.calls.asked, []);
  });

  it('asks which way to go, naming the platform and what is missing', async () => {
    const h = harness({ checks: [MISSING, READY] });
    await ensureWebDeps(h.opts);
    assert.equal(h.calls.asked.length, 1);
    const q = h.calls.asked[0];
    assert.match(q.question, /bun/);
    assert.match(q.question, /macOS arm64/);
    assert.deepEqual(q.choices.map((c) => c.value), ['install', 'manual']);
    assert.match(q.choices[0].label, /tandem install/i);
    assert.match(q.choices[1].label, /myself/i);
    assert.ok(q.scriptPath.endsWith('install-web-deps.sh'));
  });

  it('writes the generated script (owner-only) and runs it when the user lets tandem install', async () => {
    const h = harness({ checks: [MISSING, READY], answer: 'install' });
    const r = await ensureWebDeps(h.opts);
    const path = join(h.stateDir, 'install-web-deps.sh');
    assert.match(readFileSync(path, 'utf8'), /curl -fsSL https:\/\/bun\.com\/install \| bash/);
    assert.equal(statSync(path).mode & 0o777, 0o700);
    assert.deepEqual(h.calls.ran, [['bash', path]]);
    assert.equal(r.action, 'installed');
    assert.equal(r.ok, true);
  });

  it('verifies the install and fails clearly if bun is still unusable afterwards', async () => {
    const h = harness({ checks: [MISSING, MISSING], answer: 'install' });
    const r = await ensureWebDeps(h.opts);
    assert.equal(r.ok, false);
    assert.equal(r.action, 'failed');
    assert.match(h.out.join('\n'), /still not available|did not work|could not verify/i);
  });

  it('reports a failing installer script and its exit status', async () => {
    const h = harness({ checks: [MISSING, MISSING], answer: 'install', runStatus: 7 });
    const r = await ensureWebDeps(h.opts);
    assert.equal(r.ok, false);
    assert.equal(r.action, 'failed');
    assert.match(h.out.join('\n'), /exit(ed)? (status|code)? ?7|status 7/i);
  });

  it('keeps the script for review and prints the manual commands when the user will install it themselves', async () => {
    const h = harness({ checks: [MISSING], answer: 'manual' });
    const r = await ensureWebDeps(h.opts);
    assert.equal(r.action, 'manual');
    assert.equal(r.ok, false);
    assert.deepEqual(h.calls.ran, []);
    const text = h.out.join('\n');
    assert.match(text, /curl -fsSL https:\/\/bun\.com\/install \| bash/);
    assert.match(text, /install-web-deps\.sh/);
    assert.match(text, /run .*again|re-run/i);
  });

  it('installs without asking when --yes was given', async () => {
    const h = harness({ checks: [MISSING, READY], yes: true });
    const r = await ensureWebDeps(h.opts);
    assert.deepEqual(h.calls.asked, []);
    assert.equal(r.action, 'installed');
  });

  it('never installs when installing is disabled, and explains how to do it manually', async () => {
    const h = harness({ checks: [MISSING], noInstall: true, yes: true });
    const r = await ensureWebDeps(h.opts);
    assert.deepEqual(h.calls.asked, []);
    assert.deepEqual(h.calls.ran, []);
    assert.equal(r.action, 'manual');
    assert.match(h.out.join('\n'), /bun\.com\/install/);
  });

  it('does not hang or install when there is no terminal to ask on', async () => {
    const h = harness({ checks: [MISSING], isTty: false });
    const r = await ensureWebDeps(h.opts);
    assert.deepEqual(h.calls.asked, []);
    assert.deepEqual(h.calls.ran, []);
    assert.equal(r.action, 'manual');
    assert.match(h.out.join('\n'), /--yes/);
  });

  it('does not offer to install on an unsupported platform, it explains why', async () => {
    const unsupported = { facts: facts({ platform: { os: 'freebsd', arch: 'riscv64', libc: null } }), bunPath: null, bunVersion: null };
    const h = harness({ checks: [unsupported] });
    const r = await ensureWebDeps(h.opts);
    assert.equal(r.action, 'unsupported');
    assert.deepEqual(h.calls.asked, []);
    assert.match(h.out.join('\n'), /freebsd riscv64/);
    assert.match(h.out.join('\n'), /bun\.com\/docs\/installation/);
  });

  it('explains missing prerequisites instead of offering an install that cannot work', async () => {
    const noUnzip = { facts: facts({ platform: LINUX_X64, tools: ['curl', 'apt'] }), bunPath: null, bunVersion: null };
    const h = harness({ checks: [noUnzip] });
    const r = await ensureWebDeps(h.opts);
    assert.equal(r.action, 'manual');
    assert.deepEqual(h.calls.asked, []);
    assert.match(h.out.join('\n'), /sudo apt install unzip/);
  });

  it('runs PowerShell scripts with powershell on Windows', async () => {
    const win = { facts: facts({ platform: { os: 'windows', arch: 'x64', libc: null }, tools: [] }), bunPath: null, bunVersion: null };
    const h = harness({ checks: [win, READY], answer: 'install' });
    await ensureWebDeps(h.opts);
    const [cmd, ...args] = h.calls.ran[0];
    assert.equal(cmd, 'powershell');
    assert.ok(args.includes('-File'));
    assert.ok(args.at(-1).endsWith('install-web-deps.ps1'));
  });

  it('treats ttyd alone as optional: informs but does not block', async () => {
    const ttydOnly = { facts: facts({ bun: 'ok', needTerminal: true, tools: ['apt'], platform: LINUX_X64 }), bunPath: '/b/bun', bunVersion: '1' };
    const h = harness({ checks: [ttydOnly] });
    const r = await ensureWebDeps(h.opts);
    assert.equal(r.ok, true);
    assert.equal(r.action, 'present');
    assert.deepEqual(h.calls.asked, []);
    assert.match(h.out.join('\n'), /ttyd/);
    assert.match(h.out.join('\n'), /sudo apt install ttyd/);
  });
});

describe('cross-implementation parity fixture', () => {
  const cases = JSON.parse(
    readFileSync(new URL('../tandem/tests/fixtures/parity/web-deps.json', import.meta.url), 'utf8'),
  );

  it('covers every supported platform and the failure modes', () => {
    const names = cases.map((c) => c.name).join(' ');
    for (const needle of ['macos', 'linux', 'windows', 'musl', 'unsupported', 'unzip', 'ttyd', 'broken', 'sse42', 'too-old']) {
      assert.ok(names.includes(needle), `fixture should cover ${needle}`);
    }
  });

  for (const c of cases) {
    it(`Node planner matches the shared fixture: ${c.name}`, () => {
      assert.deepEqual(planFromFacts(c.facts), c.expected);
    });
  }
});
