import { afterAll, beforeAll } from "bun:test";
import { GlobalRegistrator } from "@happy-dom/global-registrator";

export function useDom(): void {
  beforeAll(() => {
    if (typeof globalThis.window === "undefined") GlobalRegistrator.register();
  });
  afterAll(async () => {
    if (typeof globalThis.window !== "undefined") await GlobalRegistrator.unregister();
  });
}
