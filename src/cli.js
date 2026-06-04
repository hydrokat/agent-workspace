import { readFileSync } from 'node:fs';
import { fileURLToPath } from 'node:url';
import { dirname, join } from 'node:path';

const __dirname = dirname(fileURLToPath(import.meta.url));

const USAGE = `
agent-workspace — initialize and maintain an AI agent workspace

Usage:
  agent-workspace <command> [options]

Commands:
  init      Initialize the current directory as an agent workspace
  update    Update an existing workspace with latest skills, agents, and guidelines
  doctor    Verify workspace correctness

Options:
  --yes                  Non-interactive; accept all defaults
  --providers <list>     Comma-separated providers: claude,antigravity,codex (default: claude)
  --name <name>          Project name
  --no-link              Skip sibling codebase linking
  --link-all             Link all sibling codebases without prompting
  --quiet                Suppress per-file output
  --verbose              Show all actions including unchanged files
  --json                 Emit machine-readable JSON output
  --dry-run              Show what would change without writing anything
  --version              Print version
  --help                 Print this help

Examples:
  npx hydrokat/agent-workspace init
  npx hydrokat/agent-workspace init --yes --providers claude,codex --name "my-project"
  npx hydrokat/agent-workspace update
  npx hydrokat/agent-workspace doctor
`.trim();

function parseArgs(argv) {
  const flags = {
    yes: false,
    providers: null,
    name: null,
    noLink: false,
    linkAll: false,
    quiet: false,
    verbose: false,
    json: false,
    dryRun: false,
    version: false,
    help: false,
  };
  const positional = [];

  for (let i = 0; i < argv.length; i++) {
    const arg = argv[i];
    if (arg === '--yes' || arg === '-y') { flags.yes = true; }
    else if (arg === '--no-link') { flags.noLink = true; }
    else if (arg === '--link-all') { flags.linkAll = true; }
    else if (arg === '--quiet' || arg === '-q') { flags.quiet = true; }
    else if (arg === '--verbose' || arg === '-v') { flags.verbose = true; }
    else if (arg === '--json') { flags.json = true; }
    else if (arg === '--dry-run') { flags.dryRun = true; }
    else if (arg === '--version') { flags.version = true; }
    else if (arg === '--help' || arg === '-h') { flags.help = true; }
    else if (arg === '--providers' && argv[i + 1]) { flags.providers = argv[++i].split(',').map(p => p.trim()); }
    else if (arg.startsWith('--providers=')) { flags.providers = arg.slice(12).split(',').map(p => p.trim()); }
    else if (arg === '--name' && argv[i + 1]) { flags.name = argv[++i]; }
    else if (arg.startsWith('--name=')) { flags.name = arg.slice(7); }
    else if (!arg.startsWith('--')) { positional.push(arg); }
  }

  return { flags, positional };
}

export async function main(argv) {
  const { flags, positional } = parseArgs(argv);

  if (flags.version) {
    const pkg = JSON.parse(readFileSync(join(__dirname, '..', 'package.json'), 'utf8'));
    console.log(pkg.version);
    return;
  }

  const command = positional[0];

  if (flags.help || !command) {
    console.log(USAGE);
    if (!command && !flags.help) process.exit(1);
    return;
  }

  const context = { flags, cwd: process.cwd() };

  switch (command) {
    case 'init': {
      const { run } = await import('./commands/init.js');
      await run(context);
      break;
    }
    case 'update': {
      const { run } = await import('./commands/update.js');
      await run(context);
      break;
    }
    case 'doctor': {
      const { run } = await import('./commands/doctor.js');
      const ok = await run(context);
      if (!ok) process.exit(1);
      break;
    }
    default:
      console.error(`Unknown command: ${command}\n`);
      console.log(USAGE);
      process.exit(1);
  }
}
