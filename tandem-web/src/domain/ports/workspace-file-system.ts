import type { FileKind } from "../models/file-entry";

export interface RawEntry {
  name: string;
  kind: FileKind;
  size: number | null;
  modifiedMs: number | null;
  resolvedPath: string | null;
  resolvedKind: FileKind | null;
}

export interface OpenedFile {
  name: string;
  size: number;
  stream(): ReadableStream<Uint8Array>;
  readHead(maxBytes: number): Promise<Uint8Array>;
  close(): Promise<void>;
}

export interface IWorkspaceFileSystem {
  allowedRoots(workspacePath: string): Promise<string[]>;
  realpath(path: string): Promise<string>;
  list(realDir: string, limit: number): Promise<{ entries: RawEntry[]; truncated: boolean }>;
  openFile(realPath: string): Promise<OpenedFile>;
}
