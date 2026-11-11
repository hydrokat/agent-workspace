import { isAbsolute, join } from "node:path";

/**
 * Returns the tandem state directory, following XDG conventions and
 * matching the Rust `registry::resolve_state_dir()` logic exactly.
 *
 * Rules (identical to the Rust implementation):
 * - If XDG_STATE_HOME is set, non-empty, and absolute → join it with "tandem"
 * - Otherwise fall back to $HOME/.local/state/tandem (or /.local/state/tandem)
 *
 * Uses process.env.HOME directly (like the Rust code uses std::env::var("HOME"))
 * rather than os.homedir() so the value is predictable in tests.
 */
export function resolveStateDir(): string {
  const xdg = process.env.XDG_STATE_HOME;
  if (xdg && isAbsolute(xdg)) {
    return join(xdg, "tandem");
  }
  const home = process.env.HOME || "/";
  return join(home, ".local", "state", "tandem");
}
