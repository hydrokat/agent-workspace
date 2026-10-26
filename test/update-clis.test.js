import { describe, it, beforeEach, afterEach } from 'node:test';
import assert from 'node:assert/strict';
import { chmodSync, copyFileSync, mkdirSync, readFileSync, existsSync, symlinkSync, writeFileSync } from 'node:fs';
import { join, dirname } from 'node:path';
import { fileURLToPath } from 'node:url';
import { makeTmp, cleanup, runCli, runCliWithInput } from './helpers.js';

const ROOT = dirname(dirname(fileURLToPath(import.meta.url)));
const STUB = join(ROOT, 'test/fixtures/ai-clis/stub.sh');

describe('update: AI CLIs step', () => {
  let dir; let ws; let bin; let state; let env;

  const calls = () => (existsSync(join(state, 'calls.log')) ? readFileSync(join(state, 'calls.log'), 'utf8').trim().split('\n') : []);
  const updates = () => calls().filter((c) => /^(claude|agy) update$|^npm install|^brew upgrade/.test(c));

  function install(name, version, next) {
    symlinkSync(join(state, 'stub.sh'), join(bin, name));
    writeFileSync(join(state, `${name}.version`), `${name} ${version}\n`);
    if (next) writeFileSync(join(state, `${name}.next`), `${name} ${next}\n`);
  }

  function installCodexViaNpm(version, next, latest) {
    const real = join(state, 'npmroot/lib/node_modules/@openai/codex/bin');
    mkdirSync(real, { recursive: true });
    copyFileSync(STUB, join(real, 'codex'));
    chmodSync(join(real, 'codex'), 0o755);
    symlinkSync(join(real, 'codex'), join(bin, 'codex'));
    writeFileSync(join(state, 'codex.version'), `codex-cli ${version}\n`);
    writeFileSync(join(state, 'codex.next'), `codex-cli ${next}\n`);
    for (const n of ['npm']) symlinkSync(join(state, 'stub.sh'), join(bin, n));
    writeFileSync(join(state, 'npm.latest'), `${latest}\n`);
  }

  async function init(providers = 'claude') {
    ws = join(dir, 'ws');
    mkdirSync(ws);
    const { code } = await runCli(['init', '--yes', '--providers', providers, '--no-link', '--no-git'], ws);
    assert.equal(code, 0);
  }

  const update = (args, input, extra = {}) => (input === undefined
    ? runCliWithInput(['update', '--no-link', ...args], ws, '', { ...env, ...extra })
    : runCliWithInput(['update', '--no-link', ...args], ws, input, { ...env, ...extra }));

  beforeEach(() => {
    dir = makeTmp();
    bin = join(dir, 'bin'); state = join(dir, 'state');
    mkdirSync(bin); mkdirSync(state);
    copyFileSync(STUB, join(state, 'stub.sh'));
    chmodSync(join(state, 'stub.sh'), 0o755);
    env = { PATH: `${bin}:/usr/bin:/bin`, STATE: state, HOME: dir };
  });
  afterEach(() => cleanup(dir));

  it('prints nothing about AI CLIs when none is installed', async () => {
    await init();
    const { code, stdout } = await update(['--yes']);
    assert.equal(code, 0);
    assert.ok(!stdout.includes('AI CLIs'));
    assert.deepEqual(updates(), []);
  });

  it('applies updates for installed CLIs with --update-clis', async () => {
    await init();
    install('claude', '2.1.305', '2.1.310');
    const { code, stdout } = await update(['--update-clis']);
    assert.equal(code, 0);
    assert.deepEqual(updates(), ['claude update']);
    assert.match(stdout, /▶ AI CLIs/);
    assert.match(stdout, /✔ claude +2\.1\.305 → 2\.1\.310/);
    assert.match(stdout, /AI CLIs: claude 2\.1\.305 → 2\.1\.310/);
  });

  it('--yes applies without asking and covers claude and agy', async () => {
    await init();
    install('claude', '2.1.305', '2.1.310');
    install('agy', '1.3.6');
    const { code, stdout } = await update(['--yes']);
    assert.equal(code, 0);
    assert.deepEqual(updates().sort(), ['agy update', 'claude update']);
    assert.match(stdout, /✔ agy +1\.3\.6 \(up to date\)/);
  });

  it('--no-update-clis wins over --yes', async () => {
    await init();
    install('claude', '2.1.305', '2.1.310');
    const { code, stdout } = await update(['--yes', '--no-update-clis']);
    assert.equal(code, 0);
    assert.deepEqual(updates(), []);
    assert.match(stdout, /AI CLI updates skipped/);
  });

  it('--dry-run lists what would run and changes nothing', async () => {
    await init();
    install('claude', '2.1.305', '2.1.310');
    const { code, stdout } = await update(['--dry-run', '--update-clis']);
    assert.equal(code, 0);
    assert.deepEqual(updates(), []);
    assert.match(stdout, /claude +2\.1\.305.*claude update/);
    assert.match(stdout, /dry run/i);
  });

  it('without a terminal or a flag it only says how to apply', async () => {
    await init();
    install('claude', '2.1.305', '2.1.310');
    install('agy', '1.3.6');
    const { code, stdout } = await update([]);
    assert.equal(code, 0);
    assert.deepEqual(updates(), []);
    assert.match(stdout, /AI CLIs: 2 can be checked; re-run with --update-clis to apply\./);
  });

  it('asks once when interactive and applies only on yes', async () => {
    await init();
    install('claude', '2.1.305', '2.1.310');
    install('agy', '1.3.6');
    const interactive = { AGENT_WORKSPACE_INTERACTIVE: '1' };
    const no = await update([], 'n\n', interactive);
    assert.equal(no.code, 0);
    assert.deepEqual(updates(), []);
    assert.equal((no.stdout.match(/Check and apply these updates\?/g) ?? []).length, 1);
    const yes = await update([], 'y\n', interactive);
    assert.equal(yes.code, 0);
    assert.deepEqual(updates().sort(), ['agy update', 'claude update']);
  });

  it('updates codex through npm only when the registry has a newer version', async () => {
    await init('codex');
    installCodexViaNpm('0.52.0', '0.54.1', '0.54.1');
    const { code, stdout } = await update(['--yes']);
    assert.equal(code, 0);
    assert.match(stdout, /codex +0\.52\.0 .*0\.54\.1 available/);
    assert.ok(updates().includes('npm install -g @openai/codex@latest'));
    assert.match(stdout, /✔ codex +0\.52\.0 → 0\.54\.1/);
  });

  it('leaves out CLIs that are already current', async () => {
    await init('codex');
    installCodexViaNpm('0.54.1', '0.54.1', '0.54.1');
    const { code, stdout } = await update(['--yes']);
    assert.equal(code, 0);
    assert.ok(!updates().some((c) => c.startsWith('npm install')));
    assert.match(stdout, /up to date: codex 0\.54\.1/);
  });

  it('reports a failing updater, keeps going, and exits 0', async () => {
    await init('codex');
    install('claude', '2.1.305', '2.1.310');
    installCodexViaNpm('0.52.0', '0.54.1', '0.54.1');
    writeFileSync(join(state, 'npm.install.fail'), '');
    const { code, stdout } = await update(['--yes']);
    assert.equal(code, 0);
    assert.match(stdout, /✖ codex +update failed.*npm install -g @openai\/codex@latest/);
    assert.match(stdout, /✔ claude +2\.1\.305 → 2\.1\.310/);
  });

  it('does not run claude update when DISABLE_UPDATES is set', async () => {
    await init();
    install('claude', '2.1.305', '2.1.310');
    const { code, stdout } = await update(['--yes'], undefined, { DISABLE_UPDATES: '1' });
    assert.equal(code, 0);
    assert.deepEqual(updates(), []);
    assert.match(stdout, /↑ claude +skipped.*DISABLE_UPDATES/);
  });

  it('warns about a CLI still below the tandem minimum', async () => {
    await init();
    install('claude', '2.1.200');
    const { code, stdout } = await update(['--yes']);
    assert.equal(code, 0);
    assert.match(stdout, /⚠ claude 2\.1\.200 is below tandem's minimum 2\.1\.310/);
  });

  it('emits the results as a JSON record with --json', async () => {
    await init();
    install('claude', '2.1.305', '2.1.310');
    const { code, stdout } = await update(['--json', '--update-clis']);
    assert.equal(code, 0);
    const records = stdout.trim().split('\n').map((l) => JSON.parse(l));
    const data = records.find((r) => r.level === 'data' && r.key === 'aiClis');
    assert.ok(data, 'aiClis record');
    assert.deepEqual([data.value[0].cli, data.value[0].before, data.value[0].after], ['claude', '2.1.305', '2.1.310']);
  });

  it('finishes with stdin closed and --yes (the tandem contract)', async () => {
    await init();
    install('claude', '2.1.305', '2.1.310');
    const { code } = await runCliWithInput(['update', '--yes', '--no-link'], ws, '', env);
    assert.equal(code, 0);
    assert.deepEqual(updates(), ['claude update']);
  });

  it('documents the flags in --help', async () => {
    const { stdout } = await runCli(['--help'], dir);
    assert.match(stdout, /--update-clis \/ --no-update-clis/);
  });
});
