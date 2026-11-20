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
      assert.ok(text.includes('Web'), 'Web tab');
      assert.ok(text.includes('Settings'), 'Settings tab');
    }
  });

  it('documents tab navigation keys', () => {
    for (const text of [readme, tandemReadme]) {
      assert.ok(text.includes('Shift+Tab'), 'Shift+Tab documented');
      assert.ok(text.includes('1-7'), '1-7 documented');
    }
  });
});

describe('Factory Mode documentation', () => {
  const readme = readFileSync(join(ROOT, 'README.md'), 'utf8');
  const tandemReadme = readFileSync(join(ROOT, 'tandem', 'README.md'), 'utf8');

  it('documents --factory in root README run.sh options and Factory Mode section', () => {
    assert.ok(readme.includes('--factory'), '--factory documented in README');
    assert.ok(readme.includes('Factory Mode (experimental)'), 'Factory Mode section in README');
    assert.ok(readme.includes('tandem factory'), 'tandem factory documented in README');
  });

  it('documents tandem factory commands, storage layout, and mode isolation in tandem README', () => {
    assert.ok(tandemReadme.includes('tandem factory status'), 'tandem factory status documented');
    assert.ok(tandemReadme.includes('tandem factory idea new'), 'tandem factory idea new documented');
    assert.ok(tandemReadme.includes('.agent-workspace/factory/'), 'factory storage layout documented');
    assert.ok(tandemReadme.includes('tandem-session-factory.json'), 'factory session file documented');
    assert.ok(tandemReadme.includes('Alt+f'), 'Factory tab shortcut documented');
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
      const rel = file.replace(ROOT + '/', '');
      const content = readFileSync(file, 'utf8');
      const lines = content.split('\n');
      lines.forEach((line, idx) => {
        if (pattern.test(line)) {
          const isFactoryTargetSkillOrLockfile =
            rel.startsWith('tandem/src/factory/') &&
            (line.includes('run_tests.py') ||
              line.includes('specs.py') ||
              line.includes('uv.lock') ||
              line.includes('"python3"'));
          if (!isFactoryTargetSkillOrLockfile) {
            violations.push(`${rel}:${idx + 1}: ${line.trim()}`);
          }
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

describe('factory mode ideation documentation', () => {
  const readme = readFileSync(join(ROOT, 'README.md'), 'utf8');
  const tandemReadme = readFileSync(join(ROOT, 'tandem/README.md'), 'utf8');
  const changelog = readFileSync(join(ROOT, 'CHANGELOG.md'), 'utf8');

  it('documents the ideation walkthrough and governance limit in root README', () => {
    assert.ok(readme.includes('Export team activity as CSV'), 'example idea in root README');
    assert.ok(readme.includes('ready-for-development'), 'ready tag in root README');
    assert.ok(readme.includes('guard-rail') || readme.includes('audit trail'), 'user-only limit in root README');
  });

  it('documents all ideation CLI commands, plan sections, and exit codes in tandem README', () => {
    for (const cmd of [
      'idea start',
      'idea resume',
      'idea describe',
      'idea assume',
      'idea select-direction',
      'idea ready',
      'idea revoke-ready',
      'artifact put',
      'artifact show',
      'question check',
    ]) {
      assert.ok(tandemReadme.includes(cmd), `${cmd} in tandem/README.md`);
    }
    for (const sec of ['Scope', 'Acceptance criteria', 'Risks', 'Estimates', 'Workflow recommendation']) {
      assert.ok(tandemReadme.includes(sec), `plan section ${sec} in tandem/README.md`);
    }
  });

  it('records Factory Mode ideation under Unreleased in CHANGELOG.md', () => {
    assert.ok(changelog.includes('## [Unreleased]'));
    assert.ok(changelog.includes('Factory Mode ideation'));
    assert.ok(changelog.includes('workflow engine'));
  });

  it('documents workflow keys, vocabularies, W001-W013, and run simulate in READMEs', () => {
    assert.ok(readme.includes('run simulate'), 'run simulate in README.md');
    assert.ok(readme.includes('queued'), 'queued state in README.md');
    for (const cmd of [
      'workflow validate',
      'workflow list',
      'workflow default',
      'run simulate',
      'run create',
      'run show',
      'run list',
      'run withdraw',
    ]) {
      assert.ok(tandemReadme.includes(cmd), `${cmd} in tandem/README.md`);
    }
    for (let i = 1; i <= 13; i += 1) {
      const code = `W${String(i).padStart(3, '0')}`;
      assert.ok(tandemReadme.includes(code), `${code} in tandem/README.md`);
    }
    for (const term of ['orchestrator', 'diff_review', 'ui_changed', 'human_approval', 'user-owned']) {
      assert.ok(tandemReadme.includes(term), `${term} in tandem/README.md`);
    }
  });
});

describe('factory mode execution documentation', () => {
  const readme = readFileSync(join(ROOT, 'README.md'), 'utf8');
  const tandemReadme = readFileSync(join(ROOT, 'tandem/README.md'), 'utf8');
  const changelog = readFileSync(join(ROOT, 'CHANGELOG.md'), 'utf8');

  it('documents interactive team walkthrough, login choice notice, and governance stance in root README', () => {
    assert.ok(readme.includes('p-017'), 'p-017 walkthrough in README.md');
    assert.ok(readme.includes('p-018'), 'p-018 governance reference in README.md');
    assert.ok(readme.includes('factory-provider-terms.md'), 'terms record link in README.md');
    assert.ok(readme.includes('Agents & sign-in'), 'Agents & sign-in panel in README.md');
    assert.ok(readme.includes('cost: not tracked'), 'cost: not tracked note in README.md');
    assert.ok(
      readme.includes('Factory Mode accelerates execution; it does not transfer accountability.'),
      'governance stance in README.md'
    );
    assert.ok(
      readme.includes('Teams keep approval, merge, deployment, and compliance responsibility.'),
      'governance accountability in README.md'
    );
  });

  it('documents login choice, no-credentials statement, and governance stance in tandem README', () => {
    for (const phrase of [
      'factory-provider-terms.md',
      'agents & sign-in',
      'assignments.json',
      'handles no credentials',
      'Factory Mode accelerates execution; it does not transfer accountability.',
      'Teams keep approval, merge, deployment, and compliance responsibility.',
    ]) {
      assert.ok(
        tandemReadme.toLowerCase().includes(phrase.toLowerCase()),
        `"${phrase}" in tandem/README.md`
      );
    }
  });

  it('documents runner loop, retries, interactive team, action requests, job layout, result schema, and worktree rules in tandem README', () => {
    for (const term of [
      'new hypothesis',
      'interactive_ready',
      'cost: not tracked',
      'action_commands',
      'input.json',
      'prompt.md',
      'result.json',
      'log.txt',
      'files_changed',
      'worktrees/factory-',
      'feat/',
    ]) {
      assert.ok(tandemReadme.includes(term), `${term} in tandem/README.md`);
    }
  });

  it('documents run execution, login, and agent CLI commands and known limits in tandem README', () => {
    for (const cmd of [
      'run pause',
      'run resume',
      'run cancel',
      'run retry',
      'run target',
      'run cleanup',
      'run log',
      'login status',
      'login open',
      'agent assign',
      'agent list',
      'agent send',
      'agent terminate',
      'agent relaunch',
    ]) {
      assert.ok(tandemReadme.includes(cmd), `${cmd} in tandem/README.md`);
    }
    assert.ok(
      tandemReadme.toLowerCase().includes('child processes may survive'),
      'child process kill known limit in tandem/README.md'
    );
    assert.ok(
      tandemReadme.toLowerCase().includes('cost is unknown unless the provider reports usage'),
      'unknown cost known limit in tandem/README.md'
    );
  });

  it('records Factory Mode releases in CHANGELOG.md', () => {
    assert.match(changelog, /^## \[1\.17\.0\]/m, '1.17.0 heading in CHANGELOG.md');
    assert.match(changelog, /^## \[1\.22\.0\]/m, '1.22.0 heading in CHANGELOG.md');
    assert.ok(changelog.includes('p-024'), 'p-024 in CHANGELOG.md');
  });
});

describe('factory mode quality path documentation', () => {
  const readme = readFileSync(join(ROOT, 'README.md'), 'utf8');
  const tandemReadme = readFileSync(join(ROOT, 'tandem/README.md'), 'utf8');
  const changelog = readFileSync(join(ROOT, 'CHANGELOG.md'), 'utf8');

  it('documents end-to-end quality path walkthrough through PR and handoff in root README', () => {
    for (const term of [
      'checks discover',
      'checks set',
      'create-tests',
      'develop',
      'test',
      'review',
      'demo',
      'pull request',
      'handoff',
      'p-018',
    ]) {
      assert.ok(readme.includes(term), `"${term}" in README.md`);
    }
  });

  it('documents check commands, stage contracts table, scope check, and protected paths in tandem README', () => {
    for (const term of [
      'checks discover',
      'checks set',
      'checks show',
      'create-tests',
      'develop',
      'test',
      'review',
      'demo',
      '.agent-workspace/',
      'workflows/',
      '.github/workflows/',
      '.git/hooks/',
      '.githooks/',
      'checks.json',
    ]) {
      assert.ok(tandemReadme.includes(term), `"${term}" in tandem/README.md`);
    }
  });

  it('documents review loop, evidence rules, PR preconditions/never-list, and CLI commands with exit codes in tandem README', () => {
    for (const term of [
      'max_review_rounds',
      'run evidence',
      'run prs',
      'run export-specs',
      'gh auth login',
      'GitHub',
      'one pull request per run',
      'merge',
      'approve',
      'force push',
      'protected branch',
      'stay on the machine',
    ]) {
      assert.ok(
        tandemReadme.toLowerCase().includes(term.toLowerCase()),
        `"${term}" in tandem/README.md`
      );
    }
    assert.match(tandemReadme, /exit code|exit codes/i, 'exit codes documented in tandem/README.md');
  });

  it('records Factory Mode quality path under 1.18.0 in CHANGELOG.md', () => {
    assert.match(changelog, /^## \[1\.18\.0\]/m, '1.18.0 heading in CHANGELOG.md');
    assert.ok(changelog.includes('p-017'), 'p-017 in CHANGELOG.md');
  });
});

describe('factory mode governance documentation', () => {
  const readme = readFileSync(join(ROOT, 'README.md'), 'utf8');
  const tandemReadme = readFileSync(join(ROOT, 'tandem/README.md'), 'utf8');
  const changelog = readFileSync(join(ROOT, 'CHANGELOG.md'), 'utf8');

  it('documents Governance subsection, stance, and three-gate table in root README', () => {
    assert.match(readme, /####\s+Governance\b|\*\*Governance\*\*/, 'Governance subsection in README.md');
    assert.ok(
      readme.includes('Factory Mode accelerates execution; it does not transfer accountability.'),
      'governance stance in README.md'
    );
    assert.ok(
      readme.includes('Teams keep approval, merge, deployment, and compliance responsibility.'),
      'accountability sentence in README.md'
    );
    for (const gate of ['Gate 1', 'Gate 2', 'Gate 3']) {
      assert.ok(readme.includes(gate), `${gate} in README.md`);
    }
  });

  it('documents settings, ranges, tightening rule, budgets, and telemetry in tandem README', () => {
    for (const term of [
      'max_parallel_agents',
      'default_temperature',
      'max_review_rounds',
      'approval_grace_secs',
      'only tighten',
      'not enforceable',
      'actual',
      'estimated',
      'unknown',
    ]) {
      assert.ok(
        tandemReadme.toLowerCase().includes(term.toLowerCase()),
        `"${term}" in tandem/README.md`
      );
    }
  });

  it('documents redaction, permissions, Gate 3, deploy preconditions, audit, data handling, and approval boundary in tandem README', () => {
    for (const term of [
      'screenshots',
      'raw local logs',
      'Gate 3',
      'production_deploy',
      'audit verify',
      '.env*',
      'secrets/**',
      '.git/**',
      'does not see which',
      'same OS user',
      '--via',
      'IsTerminal',
      '@tandem_role',
    ]) {
      assert.ok(
        tandemReadme.toLowerCase().includes(term.toLowerCase()),
        `"${term}" in tandem/README.md`
      );
    }
  });

  it('documents all governance CLI commands and exit codes in tandem README', () => {
    for (const cmd of [
      'settings get',
      'settings set',
      'run budget',
      'run usage',
      'run approve',
      'run data',
      'redact test',
      'permissions allow',
      'permissions deny',
      'permissions show',
      'audit verify',
    ]) {
      assert.ok(tandemReadme.includes(cmd), `${cmd} in tandem/README.md`);
    }
    assert.match(tandemReadme, /tandem factory audit\b/, 'tandem factory audit in tandem/README.md');
    assert.match(tandemReadme, /exit code/i, 'exit codes in tandem/README.md');
  });

  it('records Factory Mode governance under 1.19.0 and notes --via removal in CHANGELOG.md', () => {
    assert.match(changelog, /^## \[1\.19\.0\]/m, '1.19.0 heading in CHANGELOG.md');
    assert.ok(changelog.includes('p-018'), 'p-018 in CHANGELOG.md');
    assert.ok(changelog.includes('--via'), '--via removal noted in CHANGELOG.md');
  });
});

describe('factory mode user guide, troubleshooting, governance stance, and non-goals', () => {
  const readme = readFileSync(join(ROOT, 'README.md'), 'utf8');
  const tandemReadme = readFileSync(join(ROOT, 'tandem/README.md'), 'utf8');

  it('documents what Factory Mode is, prerequisites, walkthrough, and the three gates in root README', () => {
    for (const phrase of [
      'from idea to pull request',
      'Before you start',
      'gh auth login',
      'The three gates',
      'Gate 1',
      'Gate 2',
      'Gate 3',
      'idea revoke-ready',
      'run withdraw',
    ]) {
      assert.ok(
        readme.toLowerCase().includes(phrase.toLowerCase()),
        `"${phrase}" in README.md`
      );
    }
  });

  it('documents the pause reasons table, settings/workflows, and troubleshooting including factory doctor', () => {
    for (const phrase of [
      'When a run pauses',
      'Settings and workflows',
      'only tighten',
      'Troubleshooting',
      'mode mismatch',
      'unsupported host',
      'screenshot',
      'tandem factory doctor',
    ]) {
      assert.ok(
        readme.toLowerCase().includes(phrase.toLowerCase()),
        `"${phrase}" in README.md`
      );
    }
    assert.ok(tandemReadme.includes('tandem factory doctor'), 'tandem factory doctor in tandem/README.md');
  });

  it('documents what stays yours, the six non-goals, known limits, and your data in root README', () => {
    for (const phrase of [
      'What stays yours',
      'What Factory Mode does not do',
      'autonomous production deployment by default',
      'unrestricted provider execution',
      'cross-project portfolio optimization',
      'self-modifying workflows',
      'fully automatic merge',
      'IDE replacement',
      'Known limits',
      'same OS user',
      'grace period',
      'one pull request per run',
      'GitHub only',
      'Your data',
      '.env*',
      'secrets/**',
    ]) {
      assert.ok(
        readme.toLowerCase().includes(phrase.toLowerCase()),
        `"${phrase}" in README.md`
      );
    }
  });
});
