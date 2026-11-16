export interface WorkspaceEntry {
  id: string;
  name: string;
  path: string;
  socket: string;
  updated_at: number;
  pid?: number;
  alive?: boolean;
}

export function isValidWorkspaceId(id: string): boolean {
  return /^tandem-[a-f0-9]{10}$/.test(id);
}

export const LABEL_MAX_CHARS = 32;
export const LABEL_RULE_TEXT =
  "Letters, digits, spaces, - _ . (1 to 32 characters, starting with a letter or digit).";

export function normalizeLabel(raw: string): string {
  return raw.split(" ").filter(Boolean).join(" ");
}

export function isValidLabel(name: string): boolean {
  return (
    /^[A-Za-z0-9](?:[A-Za-z0-9._-]| (?! ))*$/.test(name) &&
    name.length <= LABEL_MAX_CHARS &&
    !name.endsWith(" ")
  );
}
