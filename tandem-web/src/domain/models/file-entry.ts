export type FileKind = "directory" | "file" | "symlink" | "other";

export interface FileEntry {
  name: string;
  path: string;
  kind: FileKind;
  size: number | null;
  modifiedMs: number | null;
  blocked: boolean;
  blockedReason?: "sensitive" | "outside";
}

export interface DirectoryListing {
  path: string;
  parent: string | null;
  entries: FileEntry[];
  truncated: boolean;
}

export interface FilePreview {
  path: string;
  size: number;
  text: string;
  truncated: boolean;
}

export const MAX_ENTRIES = 1000;
export const MAX_PREVIEW_BYTES = 256 * 1024;
export const MAX_PATH_LENGTH = 1024;

export type FileAccessErrorCode =
  | "BAD_REQUEST"
  | "OUTSIDE_WORKSPACE"
  | "SENSITIVE_PATH"
  | "NOT_FOUND"
  | "NOT_A_DIRECTORY"
  | "NOT_A_FILE"
  | "NOT_TEXT";

export class FileAccessError extends Error {
  constructor(
    public readonly code: FileAccessErrorCode,
    message: string
  ) {
    super(message);
  }
}
