import { describe, it } from 'node:test';
import assert from 'node:assert/strict';
import { readFileSync, existsSync } from 'node:fs';
import { dirname, join } from 'node:path';
import { fileURLToPath } from 'node:url';

const ROOT = dirname(dirname(fileURLToPath(import.meta.url)));
const read = (...parts) => readFileSync(join(ROOT, ...parts), 'utf8');
const pkg = JSON.parse(read('package.json'));
const webPkg = JSON.parse(read('tandem-web', 'package.json'));

describe('watch-mode scripts', () => {
  it('root test:watch watches the same files as test', () => {
    assert.ok(pkg.scripts['test:watch'], 'test:watch script exists');
    const testFiles = pkg.scripts.test.replace(/^node --test /, '');
    assert.equal(pkg.scripts['test:watch'], `node --test --watch ${testFiles}`);
  });

  it('tandem-web test:watch runs bun in watch mode', () => {
    assert.equal(webPkg.scripts['test:watch'], 'bun test --watch');
  });

  it('leaves the one-shot test scripts unchanged', () => {
    assert.match(pkg.scripts.test, /^node --test test\/baseline\.test\.js /);
    assert.equal(webPkg.scripts.test, 'bun test');
  });
});

describe('tandem/bacon.toml', () => {
  const path = join(ROOT, 'tandem', 'bacon.toml');

  it('exists', () => {
    assert.ok(existsSync(path), 'tandem/bacon.toml exists');
  });

  it('defines test and clippy jobs', () => {
    const toml = read('tandem', 'bacon.toml');
    assert.match(toml, /^\[jobs\.test\]$/m);
    assert.match(toml, /^\[jobs\.clippy\]$/m);
    assert.match(toml, /command = \["cargo", "test"[,\]]/);
    assert.match(toml, /command = \["cargo", "clippy"/);
  });

  it('exports locations to .bacon-locations automatically', () => {
    const toml = read('tandem', 'bacon.toml');
    assert.match(toml, /^\[exports\.locations\]$/m);
    assert.match(toml, /auto = true/);
    assert.match(toml, /path = "\.bacon-locations"/);
  });

  it('ignores its own output and build artifacts', () => {
    const toml = read('tandem', 'bacon.toml');
    for (const entry of ['.bacon-locations', '.bacon.socket', 'target']) {
      assert.ok(toml.includes(`"${entry}`), `${entry} ignored`);
    }
  });
});

describe('watch-mode housekeeping', () => {
  it('gitignores watcher output', () => {
    const lines = read('.gitignore').split('\n').map((l) => l.trim());
    for (const entry of ['.bacon-locations', '.bacon.socket', '.agent-workspace/test-watch/']) {
      assert.ok(lines.includes(entry), `${entry} in .gitignore`);
    }
  });

  it('documents the watch workflow in both READMEs', () => {
    for (const file of [['README.md'], ['tandem', 'README.md']]) {
      const text = read(...file);
      assert.ok(text.includes('test:watch') || text.includes('bacon'), `${file.join('/')} mentions the watch workflow`);
    }
    assert.ok(read('README.md').includes('npm run test:watch'));
    assert.ok(read('tandem', 'README.md').includes('bacon'));
  });

  it('records the watch workflow in the changelog', () => {
    assert.ok(read('CHANGELOG.md').includes('Watch-mode testing guideline'), 'CHANGELOG mentions watch mode');
  });
});
