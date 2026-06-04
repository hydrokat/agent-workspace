import { describe, it } from 'node:test';
import assert from 'node:assert/strict';
import { makeTmp, cleanup, runCli } from './helpers.js';

describe('cli', () => {
  it('--help prints usage', async () => {
    const { code, stdout } = await runCli(['--help'], process.cwd());
    assert.equal(code, 0);
    assert.ok(stdout.includes('init'), 'usage mentions init');
    assert.ok(stdout.includes('update'), 'usage mentions update');
    assert.ok(stdout.includes('doctor'), 'usage mentions doctor');
  });

  it('--version prints a version string', async () => {
    const { code, stdout } = await runCli(['--version'], process.cwd());
    assert.equal(code, 0);
    assert.match(stdout.trim(), /^\d+\.\d+\.\d+$/);
  });

  it('unknown command exits non-zero', async () => {
    const { code } = await runCli(['unknown-cmd'], process.cwd());
    assert.notEqual(code, 0);
  });

  it('no command exits non-zero', async () => {
    const { code } = await runCli([], process.cwd());
    assert.notEqual(code, 0);
  });

  it('--dry-run init writes nothing', async () => {
    const dir = makeTmp();
    try {
      const { code } = await runCli(['init', '--yes', '--dry-run', '--no-link'], dir);
      assert.equal(code, 0);
      const { readdirSync } = await import('node:fs');
      const entries = readdirSync(dir);
      assert.equal(entries.length, 0, 'dry-run writes nothing');
    } finally {
      cleanup(dir);
    }
  });
});
