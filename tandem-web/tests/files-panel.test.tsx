import { afterEach, describe, expect, it } from "bun:test";
import React from "react";
import { act, cleanup, fireEvent, render, waitFor } from "@testing-library/react";
import { useDom } from "./helpers/dom";
import { formatBytes, formatRelativeTime } from "../src/components/files/format";
import { FilesPanel } from "../src/components/files/files-panel";
import type { DirectoryListing, FilePreview } from "../src/domain/models/file-entry";

useDom();

const originalFetch = globalThis.fetch;

afterEach(() => {
  cleanup();
  globalThis.fetch = originalFetch;
});

function jsonResponse(body: unknown, status = 200): Response {
  return new Response(JSON.stringify(body), {
    status,
    headers: { "Content-Type": "application/json" },
  });
}

function makeRootListing(overrides?: Partial<DirectoryListing>): DirectoryListing {
  return {
    path: "",
    parent: null,
    truncated: false,
    entries: [
      {
        name: "specs",
        path: "specs",
        kind: "directory",
        size: null,
        modifiedMs: Date.now() - 120_000,
        blocked: false,
      },
      {
        name: "README.md",
        path: "README.md",
        kind: "file",
        size: 1536,
        modifiedMs: Date.now() - 60_000,
        blocked: false,
      },
    ],
    ...overrides,
  };
}

describe("format helpers", () => {
  it("formats byte sizes accurately", () => {
    expect(formatBytes(0)).toBe("0 B");
    expect(formatBytes(1023)).toBe("1023 B");
    expect(formatBytes(1024)).toBe("1.0 KB");
    expect(formatBytes(1536)).toBe("1.5 KB");
    expect(formatBytes(5 * 1024 * 1024)).toBe("5.0 MB");
  });

  it("formats relative times across seconds, minutes, hours, and days", () => {
    const now = 1_700_000_000_000;
    expect(formatRelativeTime(now - 25_000, now)).toContain("25 sec ago");
    expect(formatRelativeTime(now - 5 * 60_000, now)).toContain("5 min ago");
    expect(formatRelativeTime(now - 3 * 3600_000, now)).toContain("3 hr ago");
    expect(formatRelativeTime(now - 4 * 86_400_000, now)).toContain("4 days ago");
  });
});

describe("FilesPanel", () => {
  it("renders root listing and requests path= with hidden=0", async () => {
    const calls: string[] = [];
    globalThis.fetch = (async (input: RequestInfo | URL) => {
      const url = String(input);
      calls.push(url);
      return jsonResponse(makeRootListing());
    }) as unknown as typeof fetch;

    const { getByText } = render(
      <FilesPanel workspaceId="ws-1" workspaceName="Backend" />
    );

    await waitFor(() => {
      expect(getByText("specs")).toBeTruthy();
      expect(getByText("README.md")).toBeTruthy();
    });

    expect(calls[0]).toBe("/api/workspaces/ws-1/files?path=&hidden=0");
  });

  it("enters a directory and updates breadcrumb", async () => {
    const calls: string[] = [];
    globalThis.fetch = (async (input: RequestInfo | URL) => {
      const url = String(input);
      calls.push(url);
      if (url.includes("path=specs")) {
        return jsonResponse({
          path: "specs",
          parent: "",
          truncated: false,
          entries: [
            {
              name: "p-012",
              path: "specs/p-012",
              kind: "directory",
              size: null,
              modifiedMs: Date.now(),
              blocked: false,
            },
          ],
        } satisfies DirectoryListing);
      }
      return jsonResponse(makeRootListing());
    }) as unknown as typeof fetch;

    const { getByRole, getByText } = render(<FilesPanel workspaceId="ws-1" />);
    await waitFor(() => expect(getByText("specs")).toBeTruthy());

    fireEvent.click(getByRole("button", { name: /specs/i }));

    await waitFor(() => {
      expect(getByText("p-012")).toBeTruthy();
    });
    expect(calls).toContain("/api/workspaces/ws-1/files?path=specs&hidden=0");
    const nav = getByRole("navigation", { name: "Path" });
    expect(nav.textContent).toContain("specs");
  });

  it("navigates up via .. and jumps via breadcrumb segments", async () => {
    const calls: string[] = [];
    globalThis.fetch = (async (input: RequestInfo | URL) => {
      const url = String(input);
      calls.push(url);
      if (url.includes("path=specs%2Fp-012")) {
        return jsonResponse({
          path: "specs/p-012",
          parent: "specs",
          truncated: false,
          entries: [
            {
              name: "t-004.md",
              path: "specs/p-012/t-004.md",
              kind: "file",
              size: 512,
              modifiedMs: Date.now(),
              blocked: false,
            },
          ],
        } satisfies DirectoryListing);
      }
      if (url.includes("path=specs")) {
        return jsonResponse({
          path: "specs",
          parent: "",
          truncated: false,
          entries: [
            {
              name: "p-012",
              path: "specs/p-012",
              kind: "directory",
              size: null,
              modifiedMs: Date.now(),
              blocked: false,
            },
          ],
        } satisfies DirectoryListing);
      }
      return jsonResponse(makeRootListing());
    }) as unknown as typeof fetch;

    const { getByRole, getByText } = render(<FilesPanel workspaceId="ws-1" />);
    await waitFor(() => expect(getByText("specs")).toBeTruthy());
    fireEvent.click(getByRole("button", { name: /specs/i }));
    await waitFor(() => expect(getByText("p-012")).toBeTruthy());
    fireEvent.click(getByRole("button", { name: /p-012/i }));
    await waitFor(() => expect(getByText("t-004.md")).toBeTruthy());

    fireEvent.click(getByRole("button", { name: /\.\./i }));
    await waitFor(() => expect(getByText("p-012")).toBeTruthy());

    fireEvent.click(getByRole("button", { name: /p-012/i }));
    await waitFor(() => expect(getByText("t-004.md")).toBeTruthy());

    const rootCrumb = getByRole("button", { name: "Workspace root" });
    fireEvent.click(rootCrumb);
    await waitFor(() => expect(getByText("README.md")).toBeTruthy());
    expect(calls[calls.length - 1]).toBe("/api/workspaces/ws-1/files?path=&hidden=0");
  });

  it("toggles hidden files and requests hidden=1", async () => {
    const calls: string[] = [];
    globalThis.fetch = (async (input: RequestInfo | URL) => {
      const url = String(input);
      calls.push(url);
      return jsonResponse(makeRootListing());
    }) as unknown as typeof fetch;

    const { getByLabelText, getByText } = render(<FilesPanel workspaceId="ws-1" />);
    await waitFor(() => expect(getByText("specs")).toBeTruthy());

    fireEvent.click(getByLabelText("Show hidden"));

    await waitFor(() => {
      expect(calls).toContain("/api/workspaces/ws-1/files?path=&hidden=1");
    });
  });

  it("ignores slow earlier response after rapid second navigation", async () => {
    let resolveFirst!: (res: Response) => void;
    let resolveSecond!: (res: Response) => void;
    let callCount = 0;

    globalThis.fetch = ((input: RequestInfo | URL, init?: RequestInit) => {
      callCount += 1;
      const url = String(input);
      if (callCount === 1) {
        return new Promise<Response>((resolve, reject) => {
          resolveFirst = resolve;
          init?.signal?.addEventListener("abort", () => {
            reject(new DOMException("Aborted", "AbortError"));
          });
        });
      }
      if (url.includes("path=specs")) {
        return new Promise<Response>((resolve) => {
          resolveSecond = resolve;
        });
      }
      return Promise.resolve(jsonResponse(makeRootListing()));
    }) as unknown as typeof fetch;

    // Start on root, immediately resolve root, then click specs, then reload or navigate
    // Actually test workspaceId switch or rapid navigation via two folders:
    // Let's do initial root fast, then click specs (slow) and then click root crumb (fast)
    callCount = 0;
    globalThis.fetch = ((input: RequestInfo | URL, init?: RequestInit) => {
      callCount += 1;
      const url = String(input);
      if (callCount === 1) {
        return Promise.resolve(jsonResponse(makeRootListing()));
      }
      if (url.includes("path=specs")) {
        return new Promise<Response>((resolve, reject) => {
          resolveFirst = resolve;
          init?.signal?.addEventListener("abort", () => {
            reject(new DOMException("Aborted", "AbortError"));
          });
        });
      }
      return new Promise<Response>((resolve) => {
        resolveSecond = resolve;
      });
    }) as unknown as typeof fetch;

    const { getByRole, getByText, queryByText } = render(<FilesPanel workspaceId="ws-1" />);
    await waitFor(() => expect(getByText("specs")).toBeTruthy());

    fireEvent.click(getByRole("button", { name: /specs/i }));
    // Before first resolves, jump back to root via breadcrumb
    fireEvent.click(getByRole("button", { name: "Workspace root" }));

    await act(async () => {
      resolveSecond(
        jsonResponse(
          makeRootListing({
            entries: [
              {
                name: "fresh-root.txt",
                path: "fresh-root.txt",
                kind: "file",
                size: 10,
                modifiedMs: Date.now(),
                blocked: false,
              },
            ],
          })
        )
      );
    });

    await waitFor(() => expect(getByText("fresh-root.txt")).toBeTruthy());

    await act(async () => {
      resolveFirst(
        jsonResponse({
          path: "specs",
          parent: "",
          truncated: false,
          entries: [
            {
              name: "stale-spec.md",
              path: "specs/stale-spec.md",
              kind: "file",
              size: 10,
              modifiedMs: Date.now(),
              blocked: false,
            },
          ],
        } satisfies DirectoryListing)
      );
    });

    expect(queryByText("stale-spec.md")).toBeNull();
    expect(getByText("fresh-root.txt")).toBeTruthy();
  });

  it("renders blocked entries without button or download link and shows reason text", async () => {
    globalThis.fetch = (async () =>
      jsonResponse(
        makeRootListing({
          entries: [
            {
              name: ".env",
              path: ".env",
              kind: "file",
              size: 42,
              modifiedMs: Date.now(),
              blocked: true,
              blockedReason: "sensitive",
            },
            {
              name: "escape-link",
              path: "escape-link",
              kind: "symlink",
              size: null,
              modifiedMs: Date.now(),
              blocked: true,
              blockedReason: "outside",
            },
          ],
        })
      )) as unknown as typeof fetch;

    const { getByText, queryByRole } = render(<FilesPanel workspaceId="ws-1" />);
    await waitFor(() => expect(getByText(".env")).toBeTruthy());

    expect(queryByRole("button", { name: /\.env/i })).toBeNull();
    expect(queryByRole("link", { name: /Download \.env/i })).toBeNull();
    expect(getByText("Hidden for safety: looks like a secret")).toBeTruthy();
    expect(getByText("Points outside the workspace")).toBeTruthy();
  });

  it("encodes special characters in download link href using URLSearchParams", async () => {
    const weirdName = "report #1 & v2?.txt";
    globalThis.fetch = (async () =>
      jsonResponse(
        makeRootListing({
          entries: [
            {
              name: weirdName,
              path: `docs/${weirdName}`,
              kind: "file",
              size: 100,
              modifiedMs: Date.now(),
              blocked: false,
            },
          ],
        })
      )) as unknown as typeof fetch;

    const { getByRole } = render(<FilesPanel workspaceId="ws-1" />);
    const link = await waitFor(() =>
      getByRole("link", { name: `Download ${weirdName}` })
    );

    const expectedQuery = new URLSearchParams({ path: `docs/${weirdName}` }).toString();
    expect(link.getAttribute("href")).toBe(
      `/api/workspaces/ws-1/files/download?${expectedQuery}`
    );
    expect(link.getAttribute("download")).toBe(weirdName);
  });

  it("opens preview sheet as plain text and does not render HTML/XSS as DOM elements", async () => {
    const payload = `<img src=x onerror=alert(1)>`;
    globalThis.fetch = (async (input: RequestInfo | URL) => {
      const url = String(input);
      if (url.includes("/files/content")) {
        return jsonResponse({
          path: "README.md",
          size: payload.length,
          text: payload,
          truncated: false,
        } satisfies FilePreview);
      }
      return jsonResponse(makeRootListing());
    }) as unknown as typeof fetch;

    const { container, getByRole, getByText } = render(<FilesPanel workspaceId="ws-1" />);
    await waitFor(() => expect(getByText("README.md")).toBeTruthy());

    fireEvent.click(getByRole("button", { name: /README\.md/i }));

    await waitFor(() => {
      expect(getByText(payload)).toBeTruthy();
    });
    expect(container.querySelector("img")).toBeNull();
  });

  it("handles NOT_TEXT and truncated previews", async () => {
    globalThis.fetch = (async (input: RequestInfo | URL) => {
      const url = String(input);
      if (url.includes("path=image.png")) {
        return jsonResponse(
          { error: { code: "NOT_TEXT", message: "Binary file" } },
          415
        );
      }
      if (url.includes("path=large.log")) {
        return jsonResponse({
          path: "large.log",
          size: 1.5 * 1024 * 1024,
          text: "first chunk",
          truncated: true,
        } satisfies FilePreview);
      }
      return jsonResponse(
        makeRootListing({
          entries: [
            {
              name: "image.png",
              path: "image.png",
              kind: "file",
              size: 2048,
              modifiedMs: Date.now(),
              blocked: false,
            },
            {
              name: "large.log",
              path: "large.log",
              kind: "file",
              size: 1.5 * 1024 * 1024,
              modifiedMs: Date.now(),
              blocked: false,
            },
          ],
        })
      );
    }) as unknown as typeof fetch;

    const { getByRole, getByText } = render(<FilesPanel workspaceId="ws-1" />);
    await waitFor(() => expect(getByText("image.png")).toBeTruthy());

    fireEvent.click(getByRole("button", { name: /image\.png/i }));
    await waitFor(() => {
      expect(getByText("This file is not text. Download it to open it.")).toBeTruthy();
    });

    fireEvent.click(getByRole("button", { name: "Close preview" }));
    fireEvent.click(getByRole("button", { name: /large\.log/i }));
    await waitFor(() => {
      expect(getByText("Showing the first 256 KB of 1.5 MB.")).toBeTruthy();
      expect(getByText("first chunk")).toBeTruthy();
    });
  });

  it("shows mapped error messages and executes recovery actions", async () => {
    const errorCases: Array<{
      code: string;
      expectedMessage: string;
      actionLabel: string;
    }> = [
      {
        code: "NOT_FOUND",
        expectedMessage: "This folder no longer exists.",
        actionLabel: "Go to workspace root",
      },
      {
        code: "OUTSIDE_WORKSPACE",
        expectedMessage: "That location is outside this workspace.",
        actionLabel: "Go to workspace root",
      },
      {
        code: "SENSITIVE_PATH",
        expectedMessage: "This location is hidden for safety.",
        actionLabel: "Go up",
      },
      {
        code: "INTERNAL_ERROR",
        expectedMessage: "Could not load this folder.",
        actionLabel: "Retry",
      },
    ];

    for (const testCase of errorCases) {
      let shouldFail = true;
      globalThis.fetch = (async () => {
        if (shouldFail) {
          return jsonResponse(
            { error: { code: testCase.code, message: "server detail" } },
            400
          );
        }
        return jsonResponse(makeRootListing());
      }) as unknown as typeof fetch;

      const { getByRole, getByText } = render(<FilesPanel workspaceId="ws-1" />);
      await waitFor(() => expect(getByText(testCase.expectedMessage)).toBeTruthy());

      shouldFail = false;
      fireEvent.click(getByRole("button", { name: testCase.actionLabel }));
      await waitFor(() => expect(getByText("README.md")).toBeTruthy());
      cleanup();
    }

    // NETWORK error
    let failNetwork = true;
    globalThis.fetch = (async () => {
      if (failNetwork) {
        throw new TypeError("Failed to fetch");
      }
      return jsonResponse(makeRootListing());
    }) as unknown as typeof fetch;

    const { getByRole, getByText } = render(<FilesPanel workspaceId="ws-1" />);
    await waitFor(() => expect(getByText("Host unreachable.")).toBeTruthy());
    failNetwork = false;
    fireEvent.click(getByRole("button", { name: "Retry" }));
    await waitFor(() => expect(getByText("README.md")).toBeTruthy());
  });

  it("resets path to root when workspaceId changes", async () => {
    const calls: string[] = [];
    globalThis.fetch = (async (input: RequestInfo | URL) => {
      const url = String(input);
      calls.push(url);
      if (url.includes("ws-1") && url.includes("path=specs")) {
        return jsonResponse({
          path: "specs",
          parent: "",
          truncated: false,
          entries: [],
        } satisfies DirectoryListing);
      }
      return jsonResponse(makeRootListing());
    }) as unknown as typeof fetch;

    const { getByRole, getByText, rerender } = render(<FilesPanel workspaceId="ws-1" />);
    await waitFor(() => expect(getByText("specs")).toBeTruthy());

    fireEvent.click(getByRole("button", { name: /specs/i }));
    await waitFor(() =>
      expect(calls).toContain("/api/workspaces/ws-1/files?path=specs&hidden=0")
    );

    rerender(<FilesPanel workspaceId="ws-2" />);
    await waitFor(() =>
      expect(calls).toContain("/api/workspaces/ws-2/files?path=&hidden=0")
    );
  });

  it("shows 1000-item note when directory listing is truncated", async () => {
    globalThis.fetch = (async () =>
      jsonResponse(makeRootListing({ truncated: true }))) as unknown as typeof fetch;

    const { getByText } = render(<FilesPanel workspaceId="ws-1" />);
    await waitFor(() => {
      expect(getByText("Showing the first 1000 items.")).toBeTruthy();
    });
  });
});
