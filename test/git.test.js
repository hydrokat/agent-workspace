import { describe, it } from 'node:test';
import assert from 'node:assert/strict';
import { execFileSync } from 'node:child_process';
import { join } from 'node:path';
import { makeTmp, cleanup, runCli, runCliWithInput, fileExists, readFile, writeFile } from './helpers.js';

const BASE = ['--yes', '--providers', 'claude', '--no-link'];
const NONINTERACTIVE_BASE = ['--providers', 'claude', '--name', 'p', '--no-link'];

function git(dir, ...args) {
  return execFileSync('git', ['-c', 'user.email=t@t.t', '-c', 'user.name=t', ...args], { cwd: dir, encoding: 'utf8' });
}

function isRepo(dir) {
  return fileExists(dir, '.git');
}

describe('init git handling', () => {
  it('--git creates a blank local repository with no commits', async () => {
    const dir = makeTmp();
    try {
      const { code } = await runCli(['init', ...BASE, '--git'], dir);
      assert.equal(code, 0);
      assert.ok(isRepo(dir));
      assert.equal(execFileSync('git', ['rev-list', '--all', '--count'], { cwd: dir, encoding: 'utf8' }).trim(), '0');
      assert.equal(execFileSync('git', ['remote'], { cwd: dir, encoding: 'utf8' }).trim(), '');
    } finally { cleanup(dir); }
  });

  it('--yes alone does not create a repository', async () => {
    const dir = makeTmp();
    try {
      await runCli(['init', ...BASE], dir);
      assert.ok(!isRepo(dir));
    } finally { cleanup(dir); }
  });

  it('--no-git skips even when asked interactively', async () => {
    const dir = makeTmp();
    try {
      await runCliWithInput(['init', ...NONINTERACTIVE_BASE, '--no-git'], dir, 'y\n');
      assert.ok(!isRepo(dir));
    } finally { cleanup(dir); }
  });

  it('asks, and creates the repository on "y"', async () => {
    const dir = makeTmp();
    try {
      const { stdout } = await runCliWithInput(['init', ...NONINTERACTIVE_BASE], dir, 'y\n');
      assert.match(stdout, /Create a blank local one\?/);
      assert.ok(isRepo(dir));
    } finally { cleanup(dir); }
  });

  it('asks, and leaves the folder alone on "n" or empty input', async () => {
    for (const input of ['n\n', '\n']) {
      const dir = makeTmp();
      try {
        await runCliWithInput(['init', ...NONINTERACTIVE_BASE], dir, input);
        assert.ok(!isRepo(dir), JSON.stringify(input));
      } finally { cleanup(dir); }
    }
  });

  it('does not nest a repository inside another one, and does not ask', async () => {
    const parent = makeTmp();
    try {
      git(parent, 'init', '-q');
      const dir = join(parent, 'ws');
      writeFile(parent, 'ws/.keep', '');
      const { stdout } = await runCli(['init', ...BASE, '--git'], dir);
      assert.ok(!isRepo(dir));
      assert.match(stdout, /inside another git repository/);
    } finally { cleanup(parent); }
  });

  it('leaves an existing repository untouched', async () => {
    const dir = makeTmp();
    try {
      git(dir, 'init', '-q');
      const { code } = await runCli(['init', ...BASE, '--git'], dir);
      assert.equal(code, 0);
    } finally { cleanup(dir); }
  });

  it('--dry-run does not create the repository', async () => {
    const dir = makeTmp();
    try {
      await runCli(['init', ...BASE, '--git', '--dry-run'], dir);
      assert.ok(!isRepo(dir));
    } finally { cleanup(dir); }
  });

  it('keeps private tandem state out of git', async () => {
    const dir = makeTmp();
    try {
      await runCli(['init', ...BASE, '--git'], dir);
      writeFile(dir, '.agent-workspace/tandem-session.json', '{}');
      assert.ok(!git(dir, 'status', '--porcelain', '-uall').includes('tandem-session.json'));
    } finally { cleanup(dir); }
  });
});

describe('update git handling', () => {
  async function repoWorkspace() {
    const dir = makeTmp();
    await runCli(['init', ...BASE, '--git'], dir);
    git(dir, 'add', '-A');
    git(dir, 'commit', '-q', '-m', 'initial');
    return dir;
  }

  it('--commit commits only what the update changed, not the user\'s own edits', async () => {
    const dir = await repoWorkspace();
    try {
      writeFile(dir, 'run.sh', '#!/bin/sh\necho old\n');
      git(dir, 'add', '-A'); git(dir, 'commit', '-q', '-m', 'old run.sh');
      writeFile(dir, 'notes.md', 'my own work');
      writeFile(dir, 'WORKFLOW.md', 'user edit\n');
      const { code } = await runCli(['update', '--yes', '--no-link', '--commit'], dir);
      assert.equal(code, 0);
      const last = git(dir, 'log', '-1', '--format=%s').trim();
      assert.match(last, /^chore: update agent-workspace to /);
      assert.ok(git(dir, 'show', '--name-only', '--format=', 'HEAD').split('\n').includes('run.sh'));
      const left = git(dir, 'status', '--porcelain', '-uall');
      assert.match(left, /notes\.md/);
      assert.match(left, /WORKFLOW\.md/);
      assert.ok(!git(dir, 'show', '--name-only', '--format=', 'HEAD').includes('notes.md'));
    } finally { cleanup(dir); }
  });

  it('never commits .bak backups and adds no co-author line', async () => {
    const dir = await repoWorkspace();
    try {
      writeFile(dir, 'run.sh', 'changed\n');
      git(dir, 'add', '-A'); git(dir, 'commit', '-q', '-m', 'x');
      writeFile(dir, 'run.sh', 'edited again\n');
      await runCli(['update', '--yes', '--no-link', '--commit'], dir);
      assert.ok(!git(dir, 'show', '--name-only', '--format=', 'HEAD').includes('.bak'));
      assert.ok(!git(dir, 'log', '-1', '--format=%B').includes('Co-Authored-By'));
    } finally { cleanup(dir); }
  });

  it('--yes alone never commits', async () => {
    const dir = await repoWorkspace();
    try {
      writeFile(dir, 'run.sh', 'stale\n');
      git(dir, 'add', '-A'); git(dir, 'commit', '-q', '-m', 'stale');
      writeFile(dir, 'run.sh', 'stale2\n');
      const before = git(dir, 'rev-parse', 'HEAD');
      await runCli(['update', '--yes', '--no-link'], dir);
      assert.equal(git(dir, 'rev-parse', 'HEAD'), before);
    } finally { cleanup(dir); }
  });

  it('asks, and commits on "y" / skips on "n"', async () => {
    for (const [input, committed] of [['y\n', true], ['n\n', false]]) {
      const dir = await repoWorkspace();
      try {
        writeFile(dir, 'run.sh', 'drift\n');
        git(dir, 'add', '-A'); git(dir, 'commit', '-q', '-m', 'drift');
        writeFile(dir, 'run.sh', 'drift2\n');
        const before = git(dir, 'rev-parse', 'HEAD');
        const { stdout } = await runCliWithInput(['update', '--providers', 'claude', '--no-link'], dir, input);
        assert.match(stdout, /Commit the \d+ file\(s\) changed by this update/);
        assert.equal(git(dir, 'rev-parse', 'HEAD') !== before, committed, input);
      } finally { cleanup(dir); }
    }
  });

  it('warns when the branch is protected', async () => {
    const dir = await repoWorkspace();
    try {
      git(dir, 'checkout', '-q', '-B', 'trunk');
      writeFile(dir, 'run.sh', 'a\n'); git(dir, 'add', '-A'); git(dir, 'commit', '-q', '-m', 'a');
      writeFile(dir, 'run.sh', 'b\n');
      const { stdout } = await runCliWithInput(['update', '--providers', 'claude', '--no-link'], dir, 'n\n');
      assert.match(stdout, /protected branch 'trunk'/);
    } finally { cleanup(dir); }
  });

  it('an update with no real changes commits only the manifest timestamp', async () => {
    const dir = await repoWorkspace();
    try {
      await runCli(['update', '--yes', '--no-link'], dir);
      git(dir, 'add', '-A'); git(dir, 'commit', '-q', '-m', 'sync');
      await runCli(['update', '--yes', '--no-link', '--commit'], dir);
      assert.equal(git(dir, 'show', '--name-only', '--format=', 'HEAD').trim(), '.agent-workspace/manifest.json');
    } finally { cleanup(dir); }
  });

  it('--no-commit skips the question and the commit', async () => {
    const dir = await repoWorkspace();
    try {
      writeFile(dir, 'run.sh', 'q\n'); git(dir, 'add', '-A'); git(dir, 'commit', '-q', '-m', 'q');
      writeFile(dir, 'run.sh', 'r\n');
      const before = git(dir, 'rev-parse', 'HEAD');
      await runCli(['update', '--yes', '--no-link', '--no-commit'], dir);
      assert.equal(git(dir, 'rev-parse', 'HEAD'), before);
    } finally { cleanup(dir); }
  });

  it('a workspace that is not a repository just updates', async () => {
    const dir = makeTmp();
    try {
      await runCli(['init', ...BASE], dir);
      const { code, stdout } = await runCli(['update', '--yes', '--no-link', '--commit'], dir);
      assert.equal(code, 0);
      assert.match(stdout, /not its own git repository/);
      assert.ok(!fileExists(dir, '.git'));
    } finally { cleanup(dir); }
  });
});

describe('delivery guidance', () => {
  it('the spec-planner delivery task covers the workspace repo when it is a git repository', async () => {
    const dir = makeTmp();
    try {
      await runCli(['init', ...BASE], dir);
      const delivery = readFile(dir, '.agents/skills/spec-planner/assets/delivery-task.md');
      assert.match(delivery, /agent workspace itself is a git repository/i);
      assert.match(readFile(dir, '.agents/skills/spec-planner/SKILL.md'), /Workspace Is Delivered Too/);
    } finally { cleanup(dir); }
  });
});
