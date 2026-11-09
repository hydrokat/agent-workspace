export interface WorkspaceEntry {
  id: string;
  name: string;
  path: string;
  socket: string;
  updated_at: number;
  pid?: number;
}

export function isValidWorkspaceId(id: string): boolean {
  return /^tandem-[a-f0-9]{10}$/.test(id);
}

export function isValidCustomName(name: string): boolean {
  return /^[a-zA-Z0-9]{1,32}$/.test(name);
}
