import { afterEach, beforeEach, describe, expect, it, spyOn } from "bun:test";
import { execFile } from "node:child_process";
import { mkdtemp, mkdir, rm, symlink, writeFile } from "node:fs/promises";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { promisify } from "node:util";
import {
  ListDirectoryUseCase,
  OpenDownloadUseCase,
  ReadFilePreviewUseCase,
} from "../src/application/use-cases/browse-files";
import { MAX_ENTRIES, MAX_PREVIEW_BYTES } from "../src/domain/models/file-entry";
import type { IWorkspaceRegistry } from "../src/domain/ports/workspace-registry";
import { NodeWorkspaceFileSystem } from "../src/infrastructure/files/node-workspace-file-system";

const execFileAsync = promisify(execFile);

class TempRegistry implements IWorkspaceRegistry {
  constructor(private readonly wsPath: string) {}
  async listActive() {
    return [
      {
        id: "tandem-aaaaaaaaaa",
        name: "temp",
        path: this.wsPath,
        socket: "tandem-aaaaaaaaaa",
        updated_at: 1700000000,
      },
    ];
  }
  async findById(id: string) {
    return id === "tandem-aaaaaaaaaa" ? (await this.listActive())[0] : null;
  }
}

describe("NodeWorkspaceFileSystem & browse-files integration", () => {
  let baseDir: string;
  let wsDir: string;
  let outsideDir: string;
  let fsAdapter: NodeWorkspaceFileSystem;
  let registry: TempRegistry;

  beforeEach(async () => {
    baseDir = await mkdtemp(join(tmpdir(), "tandem-files-test-"));
    wsDir = join(baseDir, "ws");
    outsideDir = join(baseDir, "outside");
    await mkdir(wsDir);
    await mkdir(outsideDir);
    await writeFile(join(outsideDir, "secret.txt"), "top secret");
    fsAdapter = new NodeWorkspaceFileSystem();
    registry = new TempRegistry(wsDir);
  });

  afterEach(async () => {
    await rm(baseDir, { recursive: true, force: true });
  });

  it("1. lists directories first in natural name order with sizes and times", async () => {
    await mkdir(join(wsDir, "dir10"));
    await mkdir(join(wsDir, "dir2"));
    await writeFile(join(wsDir, "file10.txt"), "12345");
    await writeFile(join(wsDir, "file2.txt"), "12");

    const uc = new ListDirectoryUseCase(registry, fsAdapter);
    const listing = await uc.execute("tandem-aaaaaaaaaa", null, false);

    expect(listing.path).toBe("");
    expect(listing.parent).toBeNull();
    expect(listing.truncated).toBe(false);
    expect(listing.entries.map((e) => [e.name, e.kind, e.size])).toEqual([
      ["dir2", "directory", null],
      ["dir10", "directory", null],
      ["file2.txt", "file", 2],
      ["file10.txt", "file", 5],
    ]);
    for (const entry of listing.entries) {
      expect(typeof entry.modifiedMs).toBe("number");
      expect(entry.blocked).toBe(false);
    }
  });

  it("2. omits hidden entries by default and includes them with showHidden=true", async () => {
    await writeFile(join(wsDir, ".hidden"), "secret");
    await writeFile(join(wsDir, "visible.txt"), "ok");

    const uc = new ListDirectoryUseCase(registry, fsAdapter);
    const withoutHidden = await uc.execute("tandem-aaaaaaaaaa", null, false);
    expect(withoutHidden.entries.map((e) => e.name)).toEqual(["visible.txt"]);

    const withHidden = await uc.execute("tandem-aaaaaaaaaa", null, true);
    expect(withHidden.entries.map((e) => e.name)).toEqual([".hidden", "visible.txt"]);
  });

  it("3. marks symlink to outside file as blocked(outside) and rejects access", async () => {
    await symlink(join(outsideDir, "secret.txt"), join(wsDir, "escape.txt"));

    const listUc = new ListDirectoryUseCase(registry, fsAdapter);
    const listing = await listUc.execute("tandem-aaaaaaaaaa", null, false);
    expect(listing.entries).toHaveLength(1);
    expect(listing.entries[0]).toMatchObject({
      name: "escape.txt",
      blocked: true,
      blockedReason: "outside",
      size: null,
    });

    const previewUc = new ReadFilePreviewUseCase(registry, fsAdapter);
    await expect(previewUc.execute("tandem-aaaaaaaaaa", "escape.txt")).rejects.toMatchObject({
      code: "OUTSIDE_WORKSPACE",
    });
  });

  it("4. blocks listing or traversing symlink to outside directory", async () => {
    await mkdir(join(outsideDir, "nested"));
    await writeFile(join(outsideDir, "nested", "inner.txt"), "nope");
    await symlink(join(outsideDir, "nested"), join(wsDir, "link"));

    const listUc = new ListDirectoryUseCase(registry, fsAdapter);
    await expect(listUc.execute("tandem-aaaaaaaaaa", "link", false)).rejects.toMatchObject({
      code: "OUTSIDE_WORKSPACE",
    });

    const downloadUc = new OpenDownloadUseCase(registry, fsAdapter);
    await expect(downloadUc.execute("tandem-aaaaaaaaaa", "link/inner.txt")).rejects.toMatchObject({
      code: "OUTSIDE_WORKSPACE",
    });
  });

  it("5. allows browsing and downloading from symlinked directory under codebase-symlinks/", async () => {
    const linkedRepo = join(outsideDir, "linked-repo");
    await mkdir(linkedRepo);
    await writeFile(join(linkedRepo, "README.md"), "# Linked");
    await mkdir(join(wsDir, "codebase-symlinks"));
    await symlink(linkedRepo, join(wsDir, "codebase-symlinks", "repo"));

    const listUc = new ListDirectoryUseCase(registry, fsAdapter);
    const symlinksDir = await listUc.execute("tandem-aaaaaaaaaa", "codebase-symlinks", false);
    expect(symlinksDir.entries[0]).toMatchObject({
      name: "repo",
      kind: "directory",
      blocked: false,
    });

    const repoListing = await listUc.execute("tandem-aaaaaaaaaa", "codebase-symlinks/repo", false);
    expect(repoListing.entries.map((e) => e.name)).toEqual(["README.md"]);

    const downloadUc = new OpenDownloadUseCase(registry, fsAdapter);
    const file = await downloadUc.execute("tandem-aaaaaaaaaa", "codebase-symlinks/repo/README.md");
    const text = await new Response(file.stream()).text();
    expect(text).toBe("# Linked");
  });

  it("6. does not treat a symlink to a single file under codebase-symlinks/ as a root", async () => {
    await mkdir(join(wsDir, "codebase-symlinks"));
    await symlink(join(outsideDir, "secret.txt"), join(wsDir, "codebase-symlinks", "single.txt"));

    const downloadUc = new OpenDownloadUseCase(registry, fsAdapter);
    await expect(
      downloadUc.execute("tandem-aaaaaaaaaa", "codebase-symlinks/single.txt")
    ).rejects.toMatchObject({
      code: "OUTSIDE_WORKSPACE",
    });
  });

  it("7. allows internal symlink pointing inside the workspace", async () => {
    await writeFile(join(wsDir, "real.txt"), "inside");
    await symlink(join(wsDir, "real.txt"), join(wsDir, "alias.txt"));

    const previewUc = new ReadFilePreviewUseCase(registry, fsAdapter);
    const preview = await previewUc.execute("tandem-aaaaaaaaaa", "alias.txt");
    expect(preview.text).toBe("inside");
  });

  it("8. blocks .env and symlinks pointing to .env with SENSITIVE_PATH", async () => {
    await writeFile(join(wsDir, ".env"), "TOKEN=secret");
    await symlink(join(wsDir, ".env"), join(wsDir, "notes.txt"));

    const previewUc = new ReadFilePreviewUseCase(registry, fsAdapter);
    await expect(previewUc.execute("tandem-aaaaaaaaaa", ".env")).rejects.toMatchObject({
      code: "SENSITIVE_PATH",
    });
    await expect(previewUc.execute("tandem-aaaaaaaaaa", "notes.txt")).rejects.toMatchObject({
      code: "SENSITIVE_PATH",
    });
  });

  it("9. rejects a FIFO with NOT_A_FILE without hanging", async () => {
    const fifoPath = join(wsDir, "pipe");
    try {
      await execFileAsync("mkfifo", [fifoPath]);
    } catch {
      return;
    }

    const downloadUc = new OpenDownloadUseCase(registry, fsAdapter);
    await expect(downloadUc.execute("tandem-aaaaaaaaaa", "pipe")).rejects.toMatchObject({
      code: "NOT_A_FILE",
    });
  });

  it("10. fails when a resolved file is swapped for a symlink before openFile (O_NOFOLLOW)", async () => {
    const targetPath = join(wsDir, "swap.txt");
    await writeFile(targetPath, "original");
    const real = await fsAdapter.realpath(targetPath);

    await rm(targetPath);
    await symlink(join(outsideDir, "secret.txt"), targetPath);

    await expect(fsAdapter.openFile(real)).rejects.toThrow();
  });

  it("11. handles text preview, binary NUL rejection, and 256 KiB truncation", async () => {
    await writeFile(join(wsDir, "hello.txt"), "hello\nworld");
    await writeFile(join(wsDir, "bin.dat"), Buffer.from([0x41, 0x00, 0x42]));
    const large = Buffer.alloc(300 * 1024, "x");
    await writeFile(join(wsDir, "large.txt"), large);

    const previewUc = new ReadFilePreviewUseCase(registry, fsAdapter);

    const ok = await previewUc.execute("tandem-aaaaaaaaaa", "hello.txt");
    expect(ok).toEqual({ path: "hello.txt", size: 11, text: "hello\nworld", truncated: false });

    await expect(previewUc.execute("tandem-aaaaaaaaaa", "bin.dat")).rejects.toMatchObject({
      code: "NOT_TEXT",
    });

    const trunc = await previewUc.execute("tandem-aaaaaaaaaa", "large.txt");
    expect(trunc.size).toBe(300 * 1024);
    expect(trunc.text.length).toBe(MAX_PREVIEW_BYTES);
    expect(trunc.truncated).toBe(true);
  });

  it("12. caps directory listings at MAX_ENTRIES and sets truncated=true", async () => {
    const manyDir = join(wsDir, "many");
    await mkdir(manyDir);
    await Promise.all(
      Array.from({ length: MAX_ENTRIES + 5 }, (_, i) =>
        writeFile(join(manyDir, `f-${String(i).padStart(4, "0")}.txt`), "")
      )
    );

    const listUc = new ListDirectoryUseCase(registry, fsAdapter);
    const listing = await listUc.execute("tandem-aaaaaaaaaa", "many", false);
    expect(listing.entries).toHaveLength(MAX_ENTRIES);
    expect(listing.truncated).toBe(true);
  });

  it("13. streams 5 MiB download identically and closes handle on stream cancel", async () => {
    const payload = Buffer.alloc(5 * 1024 * 1024);
    for (let i = 0; i < payload.length; i += 1024) {
      payload[i] = i % 251;
    }
    await writeFile(join(wsDir, "big.bin"), payload);

    const downloadUc = new OpenDownloadUseCase(registry, fsAdapter);
    const file1 = await downloadUc.execute("tandem-aaaaaaaaaa", "big.bin");
    const received = Buffer.from(await new Response(file1.stream()).arrayBuffer());
    expect(received.equals(payload)).toBe(true);

    const file2 = await downloadUc.execute("tandem-aaaaaaaaaa", "big.bin");
    const closeSpy = spyOn(file2, "close");
    const stream = file2.stream();
    const reader = stream.getReader();
    await reader.read();
    await reader.cancel();
    expect(closeSpy).toHaveBeenCalled();
  });
});
