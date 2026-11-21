import { describe, it, before, after } from 'node:test';
import assert from 'node:assert/strict';
import { execFileSync } from 'node:child_process';
import { makeTmp, cleanup, runCli, fileExists, readFile } from './helpers.js';

let hasPython = true;
try { execFileSync('python3', ['--version'], { stdio: 'ignore' }); } catch { hasPython = false; }

describe('script-first skills ship their scripts and templates', () => {
  let dir;
  before(async () => {
    dir = makeTmp();
    await runCli(['init', '--yes', '--providers', 'claude', '--no-link'], dir);
  });
  after(() => cleanup(dir));

  const skill = (name) => readFile(dir, `.agents/skills/${name}/SKILL.md`);
  const has = (rel) => assert.ok(fileExists(dir, `.agents/skills/${rel}`), rel);

  it('workspace-cleanup calls scan_workspace.py and ships the candidate layout', () => {
    has('workspace-cleanup/scripts/scan_workspace.py');
    has('workspace-cleanup/assets/candidates.md');
    const text = skill('workspace-cleanup');
    assert.ok(text.includes('scan_workspace.py --delete'));
    assert.match(text, /never deletes on its own/);
  });

  it('pull-request keeps its description template in assets', () => {
    has('pull-request/assets/pr-body.md');
    const body = readFile(dir, '.agents/skills/pull-request/assets/pr-body.md');
    for (const heading of ['## Summary', '## Changes Made', '## Testing Plan', '## Revert Plan', '## Other Remarks']) {
      assert.ok(body.includes(heading), heading);
    }
    assert.ok(skill('pull-request').includes('assets/pr-body.md'));
    assert.ok(!skill('pull-request').includes('## Thinking Process'));
  });

  it('knowledge ships entry templates and points at them', () => {
    for (const name of ['decision', 'context-note', 'best-practice', 'business-flow']) {
      has(`knowledge/assets/${name}.md`);
      assert.ok(skill('knowledge').includes(`assets/${name}.md`), name);
    }
  });

  it('cleanup is wired into spec-implement and the delivery task', () => {
    assert.ok(skill('spec-implement').includes('/code-cleanup --diff'));
    assert.ok(skill('spec-planner').includes('/code-cleanup --diff'));
    assert.ok(readFile(dir, '.agents/skills/spec-planner/assets/delivery-task.md').includes('/code-cleanup --diff {{target}}'));
  });

  it('cleanup-tests is wired into the delivery task and spec-planner mandates', () => {
    const deliveryTask = readFile(dir, '.agents/skills/spec-planner/assets/delivery-task.md');
    assert.ok(deliveryTask.includes('/cleanup-tests'), 'delivery-task.md references /cleanup-tests');
    const plannerText = skill('spec-planner');
    assert.ok(plannerText.includes('/cleanup-tests'), 'spec-planner SKILL.md references /cleanup-tests');
    assert.ok(plannerText.includes('Test Cleanup'), 'spec-planner SKILL.md has Test Cleanup mandate');
  });

  const NEW_SKILLS = {
    'spec-explain': { command: '/spec-explain', files: ['assets/explanation.md'] },
    explain: { command: '/explain', files: ['scripts/locate.py', 'assets/technical.md', 'assets/non-technical.md'] },
    feasibility: { command: '/feasibility', files: ['assets/report.md'] },
    idea: { command: '/idea', files: ['scripts/idea.py', 'assets/result.md'] },
    'code-cleanup': { command: '/code-cleanup', files: ['scripts/comments.py', 'assets/candidates.md'] },
    'cleanup-tests': { command: '/cleanup-tests', files: [] },
    symlink: { command: '/symlink', files: ['scripts/symlink.py'] },
  };

  const PM_SKILLS = {
    'pm-new-sprint': {
      command: '/pm-new-sprint',
      files: [
        'scripts/sprints.py',
        'assets/sprint.md',
        'assets/business-requirements-document.html',
        'assets/functional-requirements-document.html',
        'assets/functional-specification-document.html',
      ],
    },
    'pm-sprint-explain': { command: '/pm-sprint-explain', files: ['assets/explanation.md'] },
    'pm-sprint-lock': { command: '/pm-sprint-lock', files: ['assets/lock-summary.md'] },
    'pm-sprint-unlock': { command: '/pm-sprint-unlock', files: ['assets/unlock-warning.md'] },
    'pm-sprint-close': { command: '/pm-sprint-close', files: ['assets/close-report.md'] },
    'pm-sprint-implement': { command: '/pm-sprint-implement', files: ['assets/execution-plan.md'] },
    'pm-sprint-status': { command: '/pm-sprint-status', files: ['assets/status.md'] },
    'pm-sprint-list': { command: '/pm-sprint-list', files: ['assets/list.md'] },
    'pm-commands': { command: '/pm-commands', files: ['assets/commands.md'] },
    'pm-backlogs': { command: '/pm-backlogs', files: ['assets/backlog.md'] },
  };

  for (const [name, spec] of Object.entries({ ...NEW_SKILLS, ...PM_SKILLS })) {
    it(`installs ${name} with its interface, scripts, and templates`, () => {
      has(`${name}/SKILL.md`);
      has(`${name}/agents/openai.yaml`);
      for (const file of spec.files) has(`${name}/${file}`);
      const text = skill(name);
      assert.match(text, new RegExp(`^---\\nname: ${name}\\n`));
      assert.ok(text.split('---')[1].includes(spec.command), 'description names the command');
      assert.ok(readFile(dir, `.agents/skills/${name}/agents/openai.yaml`).includes(`default_prompt: "${spec.command}"`));
    });
  }

  it('symlink reuses the workspace-init map writer and never overwrites links', () => {
    assert.ok(skill('symlink').includes('symlink.py --siblings'));
    assert.ok(skill('symlink').includes('workspace-init/scripts/link_codebases.py'));
    assert.match(skill('symlink'), /Never replace, retarget, or delete/);
  });

  it('the new skills call each other by path and keep their read-only mandates', () => {
    assert.ok(skill('spec-explain').includes('spec-planner/scripts/specs.py'));
    assert.ok(skill('feasibility').includes('explain/scripts/locate.py'));
    assert.ok(skill('code-cleanup').includes('explain/scripts/locate.py stack'));
    for (const name of ['spec-explain', 'explain', 'feasibility']) assert.match(skill(name), /Read-Only/);
    assert.match(skill('code-cleanup'), /Approval First/);
  });

  it('spec-planner ships specs.py with its templates and the spec skills call it', () => {
    has('spec-planner/scripts/specs.py');
    for (const name of ['spec-implement', 'spec-compile', 'spec-explain']) {
      assert.ok(skill(name).includes('spec-planner/scripts/specs.py'), name);
    }
  });

  it('ships the test-failures skill with its runner', () => {
    has('test-failures/SKILL.md');
    has('test-failures/agents/openai.yaml');
    has('test-failures/scripts/run_tests.py');
  });

  it('routes agent test runs through the failed-tests-only runner', () => {
    const runner = 'test-failures/scripts/run_tests.py';
    for (const name of ['code-cleanup', 'code-review-remediation', 'pull-request', 'spec-implement', 'pm-sprint-implement']) {
      assert.ok(skill(name).includes(runner), `${name} uses the runner`);
    }
    for (const doc of ['AGENTS.md', 'WORKFLOW.md']) {
      const text = readFile(dir, doc);
      assert.ok(text.includes(runner), `${doc} states the rule`);
      assert.match(text, /only the failures|only what failed|read only the failures/i, `${doc} says to read only failures`);
    }
  });

  it('installed scripts run from the workspace with python3', { skip: !hasPython }, () => {
    const run = (rel, ...args) => JSON.parse(execFileSync('python3', [`${dir}/.agents/skills/${rel}`, ...args], { cwd: dir, encoding: 'utf8' }));
    assert.equal(run('spec-planner/scripts/specs.py', 'next-phase').next, '001');
    const phase = run('spec-planner/scripts/specs.py', 'new-phase', '--repo', 'demo', '--title', 'Demo', '--branch', 'feat/demo', '--target', 'trunk');
    assert.equal(phase.phase, 'p-001-demo');
    const task = run('spec-planner/scripts/specs.py', 'new-task', 'p-001', '--title', 'Do it', '--type', 'Enhancement', '--paths', 'a.js');
    assert.deepEqual([task.task, task.delivery], ['t-001', 't-002']);
    assert.deepEqual(run('spec-planner/scripts/specs.py', 'waves', 'p-001').waves, [['t-001'], ['t-002']]);
    const deliveryText = readFile(dir, 'specs/p-001-demo/t-002.md');
    assert.match(deliveryText, /\/cleanup-tests`? on the tests added or changed in this phase \(`git diff trunk`\)/, 'delivery task renders /cleanup-tests with the merge target');
    assert.ok(deliveryText.indexOf('/code-cleanup --diff trunk') < deliveryText.indexOf('/cleanup-tests'), '/cleanup-tests runs after /code-cleanup');
    assert.ok(!deliveryText.includes('{{target}}'), 'no unrendered target placeholder');
    let lint;
    try {
      lint = run('spec-planner/scripts/specs.py', 'lint', 'p-001');
    } catch (error) {
      lint = JSON.parse(error.stdout);
    }
    assert.deepEqual(lint.problems.filter((problem) => problem.file === 't-002.md'), [], 'rendered delivery task has no lint problems');
    assert.equal(run('workspace-cleanup/scripts/scan_workspace.py').root.length > 0, true);
    assert.ok(Array.isArray(run('symlink/scripts/symlink.py', '--siblings').siblings));
    assert.equal(run('pm-new-sprint/scripts/sprints.py', 'next-sprint').next, '001');
    const catalog = run('pm-new-sprint/scripts/sprints.py', 'commands');
    assert.ok(Array.isArray(catalog.commands) && catalog.commands.length >= 9);
  });

  it('idea chains feasibility and spec-planner with unattended defaults and machine-readable output', () => {
    const ideaText = skill('idea');
    for (const token of [
      '/feasibility',
      '/spec-planner',
      'idea.py gate',
      '--auto',
      'idea_result',
      'idea.py mode',
      'factory.py ensure-idea',
      'factory-map',
      'select-direction',
    ]) {
      assert.ok(ideaText.includes(token), `idea SKILL.md includes ${token}`);
    }
    const orchText = skill('factory-orchestrator');
    assert.ok(orchText.includes('/idea'), 'factory-orchestrator SKILL.md references /idea');
    assert.ok(orchText.includes('ensure-idea'), 'factory-orchestrator SKILL.md references ensure-idea');
    const plannerText = skill('spec-planner');
    assert.ok(plannerText.includes('Unattended Mode'));
    assert.ok(plannerText.includes('Always ask the user to choose the working branch'));
    const feasText = skill('feasibility');
    assert.ok(feasText.includes('<!-- feasibility:'));
    assert.match(feasText, /Read-Only/);
    assert.ok(readFile(dir, '.agents/skills/feasibility/assets/report.md').includes('<!-- feasibility:'));
  });

  it('installs factory-orchestrator with valid front matter, provider sync, and AGENTS.md rule', () => {
    has('factory-orchestrator/SKILL.md');
    has('factory-orchestrator/agents/openai.yaml');
    has('factory-orchestrator/scripts/factory.py');
    assert.ok(fileExists(dir, '.claude/skills/factory-orchestrator/SKILL.md'), 'installed for claude provider');

    const text = skill('factory-orchestrator');
    assert.match(text, /^---\nname: factory-orchestrator\n/);
    assert.ok(text.split('---')[1].includes('TANDEM_ROLE=orchestrator'));
    assert.ok(text.includes('scripts/factory.py'));
    assert.match(text, /Never run `tandem factory idea ready` or `revoke-ready`/);

    const agentsMd = readFile(dir, 'AGENTS.md');
    assert.ok(agentsMd.includes('### 10. Factory Mode'));
    assert.ok(agentsMd.includes('TANDEM_ROLE'));
    assert.ok(agentsMd.includes('factory-orchestrator'));
  });

  it('installs the 6 marketing and design skills with openai.yaml and assets', () => {
    for (const [name, asset] of [
      ['ui-ux-pro-max', 'assets/design-system-brief.md'],
      ['marketing-plan', 'assets/marketing-plan.md'],
      ['pricing', 'assets/pricing-strategy.md'],
      ['sales-enablement', 'assets/sales-enablement.md'],
      ['social', 'assets/social-campaign.md'],
      ['offers', 'assets/offer-design.md'],
    ]) {
      has(`${name}/SKILL.md`);
      has(`${name}/agents/openai.yaml`);
      has(`${name}/${asset}`);
      assert.ok(fileExists(dir, `.claude/skills/${name}/SKILL.md`), `${name} claude`);
    }
  });

  it('installs factory-planner with valid front matter, provider sync, and specs.py reuse', () => {
    has('factory-planner/SKILL.md');
    has('factory-planner/agents/openai.yaml');
    assert.ok(fileExists(dir, '.claude/skills/factory-planner/SKILL.md'), 'installed for claude provider');

    const text = skill('factory-planner');
    assert.match(text, /^---\nname: factory-planner\n/);
    assert.ok(text.includes('spec-planner/scripts/specs.py'), 'reuses specs.py');
    assert.ok(text.includes('--root "$TANDEM_FACTORY_RUN_DIR/artifacts"'), 'points specs.py at artifact dir');
    assert.ok(text.includes('slices.json'), 'documents slices.json');
    assert.ok(text.includes('result.json'), 'documents result.json');
  });

  for (const name of ['factory-test-engineer', 'factory-implementer']) {
    it(`installs ${name} with valid front matter, provider sync, and spec-implement reuse`, () => {
      has(`${name}/SKILL.md`);
      has(`${name}/agents/openai.yaml`);
      assert.ok(fileExists(dir, `.claude/skills/${name}/SKILL.md`), `${name} installed for claude provider`);

      const text = skill(name);
      assert.match(text, new RegExp(`^---\\nname: ${name}\\n`));
      assert.ok(text.includes('slices.json'), `${name} reads slices.json`);
      assert.ok(text.includes('result.json'), `${name} writes result.json`);
      assert.ok(text.includes('hypothesis'), `${name} requires hypothesis on retry`);
      assert.match(text, /do not commit|never commit/i, `${name} forbids committing`);
      assert.ok(text.includes('spec-implement'), `${name} reuses spec-implement rules by reference`);
    });
  }

  it('installs factory-reviewer with valid front matter, provider sync, read-only mandate, and code-review reuse', () => {
    has('factory-reviewer/SKILL.md');
    has('factory-reviewer/agents/openai.yaml');
    assert.ok(fileExists(dir, '.claude/skills/factory-reviewer/SKILL.md'), 'installed for claude provider');

    const text = skill('factory-reviewer');
    assert.match(text, /^---\nname: factory-reviewer\n/);
    assert.ok(text.includes('repo_read'), 'mentions repo_read');
    assert.ok(text.includes('diff_review'), 'mentions diff_review');
    assert.ok(text.includes('code-review'), 'reuses code-review checklist');
    assert.ok(text.includes('findings.json'), 'writes findings.json');
    assert.ok(text.includes('result.json'), 'writes result.json');
    assert.match(text, /Read-Only/i, 'enforces read-only mandate');
  });

  it('installs factory-demo-maker with valid front matter, provider sync, and local-only screenshot contract', () => {
    has('factory-demo-maker/SKILL.md');
    has('factory-demo-maker/agents/openai.yaml');
    assert.ok(fileExists(dir, '.claude/skills/factory-demo-maker/SKILL.md'), 'installed for claude provider');

    const text = skill('factory-demo-maker');
    assert.match(text, /^---\nname: factory-demo-maker\n/);
    assert.ok(text.includes('demo.json'), 'writes demo.json');
    assert.ok(text.includes('result.json'), 'writes result.json');
    assert.match(text, /never uploaded/i, 'enforces local-only screenshots');
  });
});
