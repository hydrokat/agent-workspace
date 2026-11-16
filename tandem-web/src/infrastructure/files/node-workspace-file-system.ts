import { constants, type Dirent, type Stats, promises as fs } from "node:fs";
import path from "node:path";
import { FileAccessError, type FileKind } from "../../domain/models/file-entry";
import type {
  IWorkspaceFileSystem,
  OpenedFile,
  RawEntry,
} from "../../domain/ports/workspace-file-system";

const STREAM_CHUNK_BYTES = 64 * 1024;

function kindFromStats(stat: Stats): FileKind {
  if (stat.isDirectory()) return "directory";
  if (stat.isFile()) return "file";
  if (stat.isSymbolicLink()) return "symlink";
  return "other";
}

function isErrno(err: unknown, code: string): boolean {
  return (err as NodeJS.ErrnoException | null)?.code === code;
}

class NodeOpenedFile implements OpenedFile {
  private closed = false;

  constructor(
    public readonly name: string,
    public readonly size: number,
    private readonly handle: fs.FileHandle
  ) {}

  stream(): ReadableStream<Uint8Array> {
    let position = 0;
    const closeHandle = () => this.close();
    const handle = this.handle;

    return new ReadableStream<Uint8Array>({
      async pull(controller) {
        try {
          const buffer = new Uint8Array(STREAM_CHUNK_BYTES);
          const { bytesRead } = await handle.read(buffer, 0, buffer.length, position);
          if (bytesRead === 0) {
            await closeHandle();
            controller.close();
            return;
          }
          position += bytesRead;
          controller.enqueue(buffer.subarray(0, bytesRead));
        } catch (err) {
          await closeHandle();
          controller.error(err);
        }
      },
      async cancel() {
        await closeHandle();
      },
    });
  }

  async readHead(maxBytes: number): Promise<Uint8Array> {
    if (maxBytes <= 0) {
      return new Uint8Array(0);
    }
    const buffer = new Uint8Array(maxBytes);
    let offset = 0;
    while (offset < maxBytes) {
      const { bytesRead } = await this.handle.read(
        buffer,
        offset,
        maxBytes - offset,
        offset
      );
      if (bytesRead === 0) {
        break;
      }
      offset += bytesRead;
    }
    return offset === maxBytes ? buffer : buffer.subarray(0, offset);
  }

  async close(): Promise<void> {
    if (this.closed) {
      return;
    }
    this.closed = true;
    await this.handle.close();
  }
}

export class NodeWorkspaceFileSystem implements IWorkspaceFileSystem {
  async allowedRoots(workspacePath: string): Promise<string[]> {
    const rootReal = await this.realpath(workspacePath);
    const roots = [rootReal];
    const symlinksDir = path.join(rootReal, "codebase-symlinks");

    let dirEntries: Dirent[];
    try {
      dirEntries = await fs.readdir(symlinksDir, { withFileTypes: true });
    } catch (err) {
      if (isErrno(err, "ENOENT") || isErrno(err, "ENOTDIR")) {
        return roots;
      }
      throw err;
    }

    for (const entry of dirEntries) {
      const candidate = path.join(symlinksDir, entry.name);
      try {
        const real = await fs.realpath(candidate);
        const stat = await fs.stat(real);
        if (stat.isDirectory() && !roots.includes(real)) {
          roots.push(real);
        }
      } catch {
        continue;
      }
    }

    return roots;
  }

  async realpath(targetPath: string): Promise<string> {
    try {
      return await fs.realpath(targetPath);
    } catch (err) {
      if (isErrno(err, "ENOENT") || isErrno(err, "ENOTDIR")) {
        throw new FileAccessError("NOT_FOUND", "Path not found");
      }
      throw err;
    }
  }

  async list(
    realDir: string,
    limit: number
  ): Promise<{ entries: RawEntry[]; truncated: boolean }> {
    let dir;
    try {
      dir = await fs.opendir(realDir);
    } catch (err) {
      if (isErrno(err, "ENOENT")) {
        throw new FileAccessError("NOT_FOUND", "Directory not found");
      }
      if (isErrno(err, "ENOTDIR")) {
        throw new FileAccessError("NOT_A_DIRECTORY", "Not a directory");
      }
      throw err;
    }

    const entries: RawEntry[] = [];
    let truncated = false;
    let iteratorCompleted = false;

    try {
      for await (const dirent of dir) {
        if (entries.length >= limit) {
          truncated = true;
          break;
        }

        const entryPath = path.join(realDir, dirent.name);
        let lstat: Stats;
        try {
          lstat = await fs.lstat(entryPath);
        } catch {
          continue;
        }

        const kind = kindFromStats(lstat);
        let resolvedPath: string | null = entryPath;
        let resolvedKind: FileKind | null = kind;
        let size: number | null = kind === "file" ? lstat.size : null;
        let modifiedMs: number | null = Number.isFinite(lstat.mtimeMs)
          ? lstat.mtimeMs
          : null;

        if (kind === "symlink") {
          try {
            resolvedPath = await fs.realpath(entryPath);
            const targetStat = await fs.stat(resolvedPath);
            resolvedKind = kindFromStats(targetStat);
            size = resolvedKind === "file" ? targetStat.size : null;
            if (Number.isFinite(targetStat.mtimeMs)) {
              modifiedMs = targetStat.mtimeMs;
            }
          } catch {
            resolvedPath = null;
            resolvedKind = null;
            size = null;
          }
        }

        entries.push({
          name: dirent.name,
          kind,
          size,
          modifiedMs,
          resolvedPath,
          resolvedKind,
        });
      }
      if (!truncated) {
        iteratorCompleted = true;
      }
    } finally {
      if (!iteratorCompleted) {
        await Promise.resolve(dir.close()).catch(() => {});
      }
    }

    return { entries, truncated };
  }

  async openFile(realPath: string): Promise<OpenedFile> {
    // O_NONBLOCK prevents open() from blocking indefinitely on FIFOs before fstat() rejects them.
    const flags =
      constants.O_RDONLY | constants.O_NOFOLLOW | (constants.O_NONBLOCK ?? 0);

    let handle: fs.FileHandle;
    try {
      handle = await fs.open(realPath, flags);
    } catch (err) {
      if (isErrno(err, "ENOENT") || isErrno(err, "ENOTDIR")) {
        throw new FileAccessError("NOT_FOUND", "File not found");
      }
      if (isErrno(err, "ELOOP")) {
        throw new FileAccessError("OUTSIDE_WORKSPACE", "Symbolic link not allowed");
      }
      if (isErrno(err, "EISDIR")) {
        throw new FileAccessError("NOT_A_FILE", "Not a regular file");
      }
      throw err;
    }

    try {
      const stat = await handle.stat();
      if (!stat.isFile()) {
        await handle.close();
        throw new FileAccessError("NOT_A_FILE", "Not a regular file");
      }
      return new NodeOpenedFile(path.basename(realPath), stat.size, handle);
    } catch (err) {
      await handle.close().catch(() => {});
      throw err;
    }
  }
}
