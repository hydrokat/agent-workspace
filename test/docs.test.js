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

  it('documents the cargo source fallback and no longer requires uv', () => {
    for (const text of [readme, tandemReadme]) {
      assert.ok(text.includes('cargo'), 'cargo documented');
      assert.ok(!/PyInstaller|uv run|uv sync|\buv\b/.test(text), 'no uv or PyInstaller');
      assert.ok(text.includes('TANDEM_RUNTIME=auto|binary|source'), 'runtime modes documented');
    }
  });
});

describe('tandem (agent-workspace) branding', () => {
  const readme = readFileSync(join(ROOT, 'README.md'), 'utf8');
  const tandemReadme = readFileSync(join(ROOT, 'tandem/README.md'), 'utf8');

  it('titles the README with the combined name and explains the two commands', () => {
    assert.ok(readme.startsWith('# tandem (agent-workspace)'));
    assert.match(readme, /the command is `agent-workspace`/i);
    assert.match(readme, /`\.\/run\.sh` starts the tandem runtime/);
  });

  it('introduces the runtime as part of agent-workspace in the tandem README', () => {
    assert.match(tandemReadme.split('\n').slice(0, 4).join('\n'), /agent-workspace/);
  });

  it('never claims a tandem npm command', () => {
    for (const text of [readme, tandemReadme]) {
      assert.ok(!/npx tandem\b|npm (i|install)( -g)? tandem\b/.test(text), 'no tandem npm command');
    }
  });
});

describe('screenshot generator', () => {
  const script = readFileSync(join(ROOT, 'docs/screenshots/make_screenshots.py'), 'utf8');

  it('does not import the Python tandem or need uv', () => {
    assert.ok(!/^\s*(from|import)\s+tandem\b/m.test(script), 'no tandem import');
    assert.ok(!/uv run|-m tandem|sys\.executable.*tandem/.test(script), 'no uv or python -m tandem');
  });

  it('drives the Rust binary, found through --tandem or the release build', () => {
    assert.match(script, /--tandem/);
    assert.match(script, /tandem\/target\/release\/tandem/);
  });

  it('computes the tmux socket with the stdlib random formula tandem reproduces', () => {
    assert.match(script, /random\.Random\(str\(/);
    assert.match(script, /getrandbits\(40\)/);
  });

  it('is documented in the README with a plain python3 command', () => {
    const readme = readFileSync(join(ROOT, 'README.md'), 'utf8');
    assert.match(readme, /python3 docs\/screenshots\/make_screenshots\.py/);
    assert.ok(!/cd tandem && uv run python/.test(readme));
  });
});

describe('Status window documentation', () => {
  const readme = readFileSync(join(ROOT, 'README.md'), 'utf8');
  const tandemReadme = readFileSync(join(ROOT, 'tandem', 'README.md'), 'utf8');

  it('documents the five tabs: Status, Sprint, Spec, Task, Queries', () => {
    for (const text of [readme, tandemReadme]) {
      assert.ok(text.includes('Status'), 'Status tab');
      assert.ok(text.includes('Sprint'), 'Sprint tab');
      assert.ok(text.includes('Spec'), 'Spec tab');
      assert.ok(text.includes('Task'), 'Task tab');
      assert.ok(text.includes('Queries'), 'Queries tab');
      assert.ok(text.includes('Settings'), 'Settings tab');
    }
  });

  it('documents tab navigation keys', () => {
    for (const text of [readme, tandemReadme]) {
      assert.ok(text.includes('Shift+Tab'), 'Shift+Tab documented');
      assert.ok(text.includes('1-6') || text.includes('1-5') || text.includes('1..5'), '1-6 or 1-5 documented');
    }
  });
});

describe('README spec compilation and phase pruning documentation', () => {
  const readme = readFileSync(join(ROOT, 'README.md'), 'utf8');

  it('documents spec compile and guarded phase pruning workflow', () => {
    assert.ok(readme.includes('/spec-compile'), '/spec-compile in README');
    assert.ok(readme.includes('specs.py prune') || readme.includes('guarded phase pruning'), 'guarded phase pruning in README');
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

describe('repo-level runtime reference guard', () => {
  function getFiles(dir, ext) {
    const results = [];
    if (!existsSync(dir)) return results;
    for (const entry of readdirSync(dir, { withFileTypes: true })) {
      const full = join(dir, entry.name);
      if (entry.isDirectory()) {
        results.push(...getFiles(full, ext));
      } else if (!ext || full.endsWith(ext)) {
        results.push(full);
      }
    }
    return results;
  }

  it('prohibits python, uv, and pyproject references across runtime, installer, and core docs', () => {
    const strictTargets = [
      join(ROOT, 'run.sh'),
      join(ROOT, 'tandem/README.md'),
      ...getFiles(join(ROOT, 'bin')),
      ...getFiles(join(ROOT, 'src'), '.js'),
      ...getFiles(join(ROOT, 'tandem/src'), '.rs'),
    ];

    const pattern = /python|\buv\b|pyproject/i;
    const violations = [];

    for (const file of strictTargets) {
      const content = readFileSync(file, 'utf8');
      const lines = content.split('\n');
      lines.forEach((line, idx) => {
        if (pattern.test(line)) {
          violations.push(`${file.replace(ROOT + '/', '')}:${idx + 1}: ${line.trim()}`);
        }
      });
    }

    // Root README may only mention python3 for skill scripts or doc screenshot generation
    const readmeLines = readFileSync(join(ROOT, 'README.md'), 'utf8').split('\n');
    readmeLines.forEach((line, idx) => {
      if (pattern.test(line)) {
        const allowed =
          line.includes('.agents/skills') ||
          line.includes('docs/screenshots') ||
          line.includes('Python 3 scripts');
        if (!allowed) {
          violations.push(`README.md:${idx + 1}: ${line.trim()}`);
        }
      }
    });

    assert.equal(
      violations.length,
      0,
      `Found forbidden references to python/uv/pyproject in runtime/installer/docs:\n${violations.join('\n')}`
    );
  });
});
