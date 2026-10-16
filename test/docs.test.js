import { describe, it } from 'node:test';
import assert from 'node:assert/strict';
import { readdirSync, readFileSync, existsSync } from 'node:fs';
import { dirname, join } from 'node:path';
import { fileURLToPath } from 'node:url';

const ROOT = dirname(dirname(fileURLToPath(import.meta.url)));
const IMAGES = join(ROOT, 'docs', 'images');
const PRIVATE = [/\/Users\//, /\/home\/[a-z]/i, /\/private\//, /\/var\/folders/, /\.pyenv/, /\/tmp\//, /\.local\/state/, /[\w.+-]+@[\w-]+\.\w+/];

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
