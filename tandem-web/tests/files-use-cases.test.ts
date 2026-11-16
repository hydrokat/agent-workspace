import { describe, expect, it } from "bun:test";
import {
  ListDirectoryUseCase,
  OpenDownloadUseCase,
  ReadFilePreviewUseCase,
} from "../src/application/use-cases/browse-files";
import { FileAccessError } from "../src/domain/models/file-entry";
import type {
  IWorkspaceFileSystem,
  OpenedFile,
  RawEntry,
} from "../src/domain/ports/workspace-file-system";
import type { IWorkspaceRegistry } from "../src/domain/ports/workspace-registry";

class FakeRegistry implements IWorkspaceRegistry {
  async listActive() {
    return [
      {
        id: "tandem-1234567890",
        name: "ws",
        path: "/ws",
        socket: "tandem-1234567890",
        updated_at: 1700000000,
      },
    ];
  }
  async findById(id: string) {
    if (id === "tandem-1234567890") {
      return (await this.listActive())[0];
    }
    return null;
  }
}

class SpyFileSystem implements IWorkspaceFileSystem {
  calls: string[] = [];
  realpathResult = "/ws/file.txt";
  listEntries: RawEntry[] = [];
  fileSize = 11;
  fileBytes = new TextEncoder().encode("hello world");

  async allowedRoots(workspacePath: string): Promise<string[]> {
    this.calls.push(`allowedRoots:${workspacePath}`);
    return [workspacePath];
  }

  async realpath(p: string): Promise<string> {
    this.calls.push(`realpath:${p}`);
    return this.realpathResult;
  }

  async list(realDir: string, limit: number): Promise<{ entries: RawEntry[]; truncated: boolean }> {
    this.calls.push(`list:${realDir}:${limit}`);
    return { entries: this.listEntries, truncated: false };
  }

  async openFile(realPath: string): Promise<OpenedFile> {
    this.calls.push(`openFile:${realPath}`);
    const bytes = this.fileBytes;
    const size = this.fileSize;
    return {
      name: "file.txt",
      size,
      stream: () =>
        new ReadableStream({
          start(c) {
            c.enqueue(bytes);
            c.close();
          },
        }),
      readHead: async (maxBytes: number) => bytes.slice(0, maxBytes),
      close: async () => {
        this.calls.push("close");
      },
    };
  }
}

describe("browse-files use cases", () => {
  it("rejects invalid workspace ID before any registry or fs call", async () => {
    const registry = new FakeRegistry();
    const fs = new SpyFileSystem();
    const uc = new ListDirectoryUseCase(registry, fs);

    await expect(uc.execute("bad-id", null, false)).rejects.toMatchObject({
      code: "BAD_REQUEST",
    });
    expect(fs.calls).toEqual([]);
  });

  it("returns NOT_FOUND for unknown workspace without fs calls", async () => {
    const registry = new FakeRegistry();
    const fs = new SpyFileSystem();
    const uc = new ReadFilePreviewUseCase(registry, fs);

    await expect(uc.execute("tandem-0000000000", "a.txt")).rejects.toMatchObject({
      code: "NOT_FOUND",
    });
    expect(fs.calls).toEqual([]);
  });

  it("rejects invalid path before allowedRoots or realpath", async () => {
    const registry = new FakeRegistry();
    const fs = new SpyFileSystem();
    const uc = new OpenDownloadUseCase(registry, fs);

    await expect(uc.execute("tandem-1234567890", "../secret")).rejects.toMatchObject({
      code: "BAD_REQUEST",
    });
    expect(fs.calls).toEqual([]);
  });

  it("rejects sensitive requested path before allowedRoots or realpath", async () => {
    const registry = new FakeRegistry();
    const fs = new SpyFileSystem();
    const uc = new OpenDownloadUseCase(registry, fs);

    await expect(uc.execute("tandem-1234567890", ".env")).rejects.toMatchObject({
      code: "SENSITIVE_PATH",
    });
    expect(fs.calls).toEqual([]);
  });

  it("rejects resolved path outside allowed roots before openFile", async () => {
    const registry = new FakeRegistry();
    const fs = new SpyFileSystem();
    fs.realpathResult = "/etc/passwd";
    const uc = new OpenDownloadUseCase(registry, fs);

    await expect(uc.execute("tandem-1234567890", "link.txt")).rejects.toMatchObject({
      code: "OUTSIDE_WORKSPACE",
    });
    expect(fs.calls).toEqual(["allowedRoots:/ws", "realpath:/ws/link.txt"]);
  });

  it("rejects resolved path that is sensitive before openFile", async () => {
    const registry = new FakeRegistry();
    const fs = new SpyFileSystem();
    fs.realpathResult = "/ws/.env";
    const uc = new ReadFilePreviewUseCase(registry, fs);

    await expect(uc.execute("tandem-1234567890", "alias.txt")).rejects.toMatchObject({
      code: "SENSITIVE_PATH",
    });
    expect(fs.calls).toEqual(["allowedRoots:/ws", "realpath:/ws/alias.txt"]);
  });

  it("executes preview in expected order and closes handle", async () => {
    const registry = new FakeRegistry();
    const fs = new SpyFileSystem();
    const uc = new ReadFilePreviewUseCase(registry, fs);

    const preview = await uc.execute("tandem-1234567890", "file.txt");
    expect(preview).toEqual({
      path: "file.txt",
      size: 11,
      text: "hello world",
      truncated: false,
    });
    expect(fs.calls).toEqual([
      "allowedRoots:/ws",
      "realpath:/ws/file.txt",
      "openFile:/ws/file.txt",
      "close",
    ]);
  });
});
