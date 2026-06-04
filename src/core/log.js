let _quiet = false;
let _verbose = false;
let _json = false;
const _records = [];

export function configure({ quiet = false, verbose = false, json = false } = {}) {
  _quiet = quiet;
  _verbose = verbose;
  _json = json;
}

function emit(level, symbol, message) {
  if (_json) {
    const rec = { level, message, ts: new Date().toISOString() };
    _records.push(rec);
    console.log(JSON.stringify(rec));
    return;
  }
  if (_quiet && level === 'file') return;
  if (!_verbose && level === 'unchanged') return;
  console.log(`${symbol} ${message}`);
}

export const log = {
  step: (msg) => emit('step', '\n▶', msg),
  ok: (msg) => emit('ok', '✓', msg),
  warn: (msg) => emit('warn', '⚠', msg),
  error: (msg) => emit('error', '✗', msg),
  info: (msg) => emit('info', ' ', msg),
  created: (path) => emit('file', '+', path),
  updated: (path) => emit('file', '~', path),
  backedUp: (path) => emit('file', '•', `${path} (backed up)`),
  skipped: (path) => emit('file', '↷', `${path} (skipped)`),
  unchanged: (path) => emit('unchanged', '=', path),
  summary: (lines) => {
    if (_json) return;
    const width = Math.max(...lines.map(l => l.length), 40);
    const bar = '─'.repeat(width + 4);
    console.log(`\n┌${bar}┐`);
    for (const line of lines) console.log(`│  ${line.padEnd(width)}  │`);
    console.log(`└${bar}┘\n`);
  },
};
