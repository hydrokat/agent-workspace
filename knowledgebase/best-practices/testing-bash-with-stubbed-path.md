# Testing Bash Scripts With A Stubbed PATH

Pattern used for `run.sh` (`test/run-sh.test.js`).

- Run the script as `bash script.sh` with `PATH` set to a temp `bin/` only. Symlink just the core utilities the script needs (`sed`, `grep`, `cat`, ...), so a tool that is "missing" really is missing.
- Provide fake executables as symlinks to one generic stub that logs every call to a file and prints a scripted version banner. Per-test state is plain marker files (`uv.dryrun.fail`, `agy.noupdate`, `tmux.installfail`).
- Make installers act: a fake `brew install X` or `curl` creates the stub for `X`, so "install then continue" is testable.
- Fake `uname` to cover other operating systems; stub `sudo` as `exec "$@"` and assert it appears in the displayed command.
- Drive prompts through a flag (`--answers y,n`) instead of a TTY; assert that a missing flag with a non-terminal stdin exits instead of hanging.
- Assert on the call log (what was and was not run), exit codes, and output.
- Security cases: feed hostile config lines (`$(...)`, backticks, `;`) and assert nothing executes.

Pitfalls found: never run a real `brew` probe without `HOMEBREW_NO_AUTO_UPDATE=1` (it updated the developer's taps); write bash 3.2-compatible code (no associative arrays) because macOS ships 3.2; do not write through a symlinked utility when faking it (remove it from the symlink list first).
