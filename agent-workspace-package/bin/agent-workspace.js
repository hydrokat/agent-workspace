#!/usr/bin/env node
import('../src/cli.js').then(({ main }) => main(process.argv.slice(2))).catch((err) => {
  console.error(err.message);
  process.exit(1);
});
