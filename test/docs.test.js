import { describe, it } from 'node:test';
import assert from 'node:assert/strict';
import { readdirSync, readFileSync, existsSync } from 'node:fs';
import { dirname, join } from 'node:path';
import { fileURLToPath } from 'node:url';

const ROOT = dirname(dirname(fileURLToPath(import.meta.url)));
const IMAGES = join(ROOT, 'docs', 'images');
const PRIVATE = [/\/Users\//, /\/home\/[a-z]/i, /\/private\//, /\/var\/folders/, /\.pyenv/, /\/tmp\//, /\.local\/state/, /[\w.+-]+@[\w-]+\.\w+/];

describe('README auto-update documentation', () => {
  const readme = readFileSync(join(ROOT, 'README.md'), 'utf8');

  it('documents --no-update flag', () => {
    assert.ok(readme.includes('--no-update'), '--no-update documented in README');
  });

  it('documents TANDEM_AUTO_UPDATE env var', () => {
    assert.ok(readme.includes('TANDEM_AUTO_UPDATE'), 'TANDEM_AUTO_UPDATE documented in README');
  });

  it('documents TANDEM_UPDATE_SPEC env var', () => {
    assert.ok(readme.includes('TANDEM_UPDATE_SPEC'), 'TANDEM_UPDATE_SPEC documented in README');
  });
});

describe('README runtime documentation', () => {
  const readme = readFileSync(join(ROOT, 'README.md'), 'utf8');
  const tandemReadme = readFileSync(join(ROOT, 'tandem', 'README.md'), 'utf8');

  it('documents TANDEM_RUNTIME env var', () => {
    assert.ok(readme.includes('TANDEM_RUNTIME'), 'TANDEM_RUNTIME in root README');
    assert.ok(tandemReadme.includes('TANDEM_RUNTIME'), 'TANDEM_RUNTIME in tandem README');
  });

  it('documents TANDEM_RELEASE_BASE env var', () => {
    assert.ok(readme.includes('TANDEM_RELEASE_BASE'), 'TANDEM_RELEASE_BASE in root README');
    assert.ok(tandemReadme.includes('TANDEM_RELEASE_BASE'), 'TANDEM_RELEASE_BASE in tandem README');
  });

  it('documents --fetch-runtime flag', () => {
    assert.ok(readme.includes('--fetch-runtime'), '--fetch-runtime in root README');
    assert.ok(tandemReadme.includes('--fetch-runtime'), '--fetch-runtime in tandem README');
  });

  it('documents state bin cache directory', () => {
    assert.ok(readme.includes('tandem/bin'), 'cache path in root README');
    assert.ok(tandemReadme.includes('tandem/bin'), 'cache path in tandem README');
  });

  it('documents supported platforms and experimental Windows MSYS2 support', () => {
    for (const p of ['linux-x86_64', 'linux-arm64', 'darwin-arm64', 'darwin-x86_64', 'windows-x86_64']) {
      assert.ok(readme.includes(p), `${p} in root README`);
      assert.ok(tandemReadme.includes(p), `${p} in tandem README`);
    }
    assert.ok(readme.includes('MSYS2'), 'MSYS2 in root README');
    assert.ok(tandemReadme.includes('MSYS2'), 'MSYS2 in tandem README');
  });

  it('documents conditional uv requirement', () => {
    assert.ok(readme.includes('uv'), 'uv documented in root README');
    assert.ok(tandemReadme.includes('uv'), 'uv documented in tandem README');
  });
});

describe('README screenshots', () => {
  const svgs = existsSync(IMAGES) ? readdirSync(IMAGES).filter((f) => f.endsWith('.svg')) : [];

  it('exist and are referenced by the README', () => {
    assert.ok(svgs.length >= 4);
    const readme = readFileSync(join(ROOT, 'README.md'), 'utf8');
    for (const f of svgs) assert.ok(readme.includes(`docs/images/${f}`), `${f} referenced`);
  });

  it('contain no home-directory paths, temp paths or email addresses', () => {
    for (const f of svgs) {
      const text = readFileSync(join(IMAGES, f), 'utf8');
      for (const re of PRIVATE) assert.ok(!re.test(text), `${f} matches ${re}`);
    }
  });

  it('are plain SVG with no scripts or external references', () => {
    for (const f of svgs) {
      const text = readFileSync(join(IMAGES, f), 'utf8');
      assert.ok(!/<script|href=|xlink:|<image|@import|url\(/i.test(text), f);
    }
  });
});
