const FENCE = '---';

export function parse(content) {
  const lines = content.split('\n');
  if (lines[0].trim() !== FENCE) return { frontmatter: {}, body: content };

  const closeIdx = lines.findIndex((l, i) => i > 0 && l.trim() === FENCE);
  if (closeIdx === -1) return { frontmatter: {}, body: content };

  const fmLines = lines.slice(1, closeIdx);
  const body = lines.slice(closeIdx + 1).join('\n');
  const frontmatter = {};

  for (const line of fmLines) {
    const colonIdx = line.indexOf(':');
    if (colonIdx === -1) continue;
    const key = line.slice(0, colonIdx).trim();
    const rawVal = line.slice(colonIdx + 1).trim();
    frontmatter[key] = parseValue(rawVal);
  }

  return { frontmatter, body };
}

function parseValue(raw) {
  if (!raw) return '';
  if (raw.startsWith('[') && raw.endsWith(']')) {
    return raw
      .slice(1, -1)
      .split(',')
      .map(v => v.trim().replace(/^["']|["']$/g, ''))
      .filter(Boolean);
  }
  return raw.replace(/^["']|["']$/g, '');
}

function serializeValue(val) {
  if (Array.isArray(val)) {
    return `[${val.map(v => `"${v}"`).join(', ')}]`;
  }
  return String(val);
}

export function serialize(frontmatter, body) {
  const lines = [FENCE];
  for (const [key, val] of Object.entries(frontmatter)) {
    lines.push(`${key}: ${serializeValue(val)}`);
  }
  lines.push(FENCE);
  const normalizedBody = body.startsWith('\n') ? body : '\n' + body;
  return lines.join('\n') + normalizedBody;
}
