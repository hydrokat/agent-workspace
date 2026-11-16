import path from "node:path";
import {
  type DirectoryListing,
  type FileEntry,
  type FileKind,
  type FilePreview,
  FileAccessError,
  MAX_ENTRIES,
  MAX_PREVIEW_BYTES,
} from "../../domain/models/file-entry";
import {
  isInsideRoot,
  isSensitivePath,
  parseRelativePath,
} from "../../domain/models/file-path-rules";
import { isValidWorkspaceId } from "../../domain/models/workspace";
import type {
  IWorkspaceFileSystem,
  OpenedFile,
  RawEntry,
} from "../../domain/ports/workspace-file-system";
import type { IWorkspaceRegistry } from "../../domain/ports/workspace-registry";

const BINARY_SNIFF_BYTES = 8192;

interface LocatedPath {
  root: string;
  roots: string[];
  segments: string[];
  real: string;
}

async function resolveInside(
  files: IWorkspaceFileSystem,
  roots: string[],
  workspaceRoot: string,
  segments: string[]
): Promise<{ real: string; root: string }> {
  const candidate =
    segments.length === 0 ? workspaceRoot : path.join(workspaceRoot, ...segments);
  const real = await files.realpath(candidate);
  const matchingRoot = roots.find((r) => isInsideRoot(r, real));
  if (!matchingRoot) {
    throw new FileAccessError("OUTSIDE_WORKSPACE", "Path resolves outside workspace");
  }

  const rel = path.relative(matchingRoot, real);
  const realSegments = rel === "" ? [] : rel.split(path.sep);
  if (isSensitivePath(realSegments)) {
    throw new FileAccessError("SENSITIVE_PATH", "Access to sensitive path is denied");
  }

  return { real, root: matchingRoot };
}

async function locate(
  registry: IWorkspaceRegistry,
  files: IWorkspaceFileSystem,
  workspaceId: string,
  rawPath: string | null
): Promise<LocatedPath> {
  if (!isValidWorkspaceId(workspaceId)) {
    throw new FileAccessError("BAD_REQUEST", "Invalid workspace ID");
  }

  const workspace = await registry.findById(workspaceId);
  if (!workspace) {
    throw new FileAccessError("NOT_FOUND", "Workspace not found");
  }

  const segments = parseRelativePath(rawPath);
  if (isSensitivePath(segments)) {
    throw new FileAccessError("SENSITIVE_PATH", "Access to sensitive path is denied");
  }

  const roots = await files.allowedRoots(workspace.path);
  const { real, root } = await resolveInside(files, roots, workspace.path, segments);
  return { root, roots, segments, real };
}

function toEntries(
  rawEntries: RawEntry[],
  parentSegments: string[],
  roots: string[],
  showHidden: boolean
): FileEntry[] {
  const entries: FileEntry[] = [];

  for (const raw of rawEntries) {
    if (!showHidden && raw.name.startsWith(".")) {
      continue;
    }

    const entrySegments = [...parentSegments, raw.name];
    let blocked = false;
    let blockedReason: "sensitive" | "outside" | undefined;

    if (isSensitivePath(entrySegments)) {
      blocked = true;
      blockedReason = "sensitive";
    } else if (raw.resolvedPath !== null) {
      const matchingRoot = roots.find((r) => isInsideRoot(r, raw.resolvedPath!));
      if (!matchingRoot) {
        blocked = true;
        blockedReason = "outside";
      } else {
        const rel = path.relative(matchingRoot, raw.resolvedPath);
        const realSegments = rel === "" ? [] : rel.split(path.sep);
        if (isSensitivePath(realSegments)) {
          blocked = true;
          blockedReason = "sensitive";
        }
      }
    }

    const kind: FileKind =
      raw.kind === "symlink" && !blocked && raw.resolvedKind !== null
        ? raw.resolvedKind
        : raw.kind;

    const size = blocked || kind !== "file" ? null : raw.size;

    const entry: FileEntry = {
      name: raw.name,
      path: entrySegments.join("/"),
      kind,
      size,
      modifiedMs: raw.modifiedMs,
      blocked,
    };
    if (blockedReason) {
      entry.blockedReason = blockedReason;
    }
    entries.push(entry);
  }

  entries.sort((a, b) => {
    const aDir = a.kind === "directory" ? 0 : 1;
    const bDir = b.kind === "directory" ? 0 : 1;
    if (aDir !== bDir) {
      return aDir - bDir;
    }
    return a.name.localeCompare(b.name, undefined, { numeric: true, sensitivity: "base" });
  });

  return entries;
}

export class ListDirectoryUseCase {
  constructor(
    private readonly registry: IWorkspaceRegistry,
    private readonly files: IWorkspaceFileSystem
  ) {}

  async execute(
    workspaceId: string,
    rawPath: string | null,
    showHidden: boolean
  ): Promise<DirectoryListing> {
    const { roots, segments, real } = await locate(
      this.registry,
      this.files,
      workspaceId,
      rawPath
    );
    const { entries, truncated } = await this.files.list(real, MAX_ENTRIES);
    return {
      path: segments.join("/"),
      parent: segments.length === 0 ? null : segments.slice(0, -1).join("/"),
      entries: toEntries(entries, segments, roots, showHidden),
      truncated,
    };
  }
}

export class ReadFilePreviewUseCase {
  constructor(
    private readonly registry: IWorkspaceRegistry,
    private readonly files: IWorkspaceFileSystem
  ) {}

  async execute(workspaceId: string, rawPath: string | null): Promise<FilePreview> {
    const { segments, real } = await locate(
      this.registry,
      this.files,
      workspaceId,
      rawPath
    );
    const file = await this.files.openFile(real);
    try {
      const toRead = Math.min(file.size, MAX_PREVIEW_BYTES);
      const bytes = await file.readHead(toRead);
      const sniffLen = Math.min(bytes.length, BINARY_SNIFF_BYTES);
      for (let i = 0; i < sniffLen; i++) {
        if (bytes[i] === 0) {
          throw new FileAccessError("NOT_TEXT", "Binary file cannot be previewed");
        }
      }
      const text = new TextDecoder("utf-8", { fatal: false }).decode(bytes);
      return {
        path: segments.join("/"),
        size: file.size,
        text,
        truncated: file.size > MAX_PREVIEW_BYTES,
      };
    } finally {
      await file.close();
    }
  }
}

export class OpenDownloadUseCase {
  constructor(
    private readonly registry: IWorkspaceRegistry,
    private readonly files: IWorkspaceFileSystem
  ) {}

  async execute(workspaceId: string, rawPath: string | null): Promise<OpenedFile> {
    const { real } = await locate(this.registry, this.files, workspaceId, rawPath);
    return this.files.openFile(real);
  }
}
