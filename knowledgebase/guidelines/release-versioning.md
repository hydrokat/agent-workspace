# Release Versioning

All pull requests must declare their intended Semantic Versioning impact before
merge. Use the Semantic Versioning 2.0.0 guide as the source of truth:
https://semver.org/

## Required PR Version Impact

Every PR must identify one of these version impacts:

| Impact | When to use | Example next version |
|---|---|---|
| Major | Backward-incompatible public API or package behavior changes | `1.4.2` -> `2.0.0` |
| Minor | Backward-compatible new functionality, deprecations, or substantial compatible improvements | `1.4.2` -> `1.5.0` |
| Patch | Backward-compatible bug fixes or low-risk internal corrections | `1.4.2` -> `1.4.3` |
| Pre-release | Unstable preview, beta, or release-candidate builds | `1.5.0` -> `1.5.0-rc.1` |
| Build metadata | Build number, CI run, timestamp, or commit metadata when needed | `1.5.0` -> `1.5.0+build.42` |

If a PR is documentation-only, test-only, or otherwise does not ship a package
change, mark the version impact as `None` and explain why no package version
changes.

## Merge Requirements

Before merge, confirm that:

- The PR states the version impact and the exact next version when known.
- The selected version follows `MAJOR.MINOR.PATCH`.
- Major, minor, and patch numbers are non-negative integers with no leading
  zeroes.
- Breaking changes bump `MAJOR` and reset `MINOR` and `PATCH` to `0`.
- Backward-compatible features bump `MINOR` and reset `PATCH` to `0`.
- Backward-compatible fixes bump `PATCH`.
- Released package contents are never modified in place; follow-up fixes ship
  as a new version.

## Tagging On Merge

After a PR with a package version impact is merged, tag the merge commit with
the released version. Use `v` as the Git tag prefix while keeping the semantic
version itself unprefixed:

```bash
git tag -a v1.5.0 -m "Release version 1.5.0"
git push origin v1.5.0
```

Do not reuse or move an existing release tag. If a release was incorrect, create
a new version and tag that new version.

## Build Numbers And Metadata

Use SemVer build metadata when a build number is needed for traceability:

```text
1.5.0+build.42
1.5.0+20260824
1.5.0-rc.1+build.42
```

Build metadata starts after `+`, may contain dot-separated ASCII alphanumeric
or hyphenated identifiers, and does not change version precedence. Use it for
CI build numbers, commit identifiers, timestamps, or platform-specific package
traceability. Do not use build metadata to smuggle in compatibility changes; the
major, minor, patch, and pre-release parts must still communicate release risk.

## PR Checklist

Include this release block in PR descriptions when the change can ship in a
package:

```markdown
## Release Versioning

Version impact: Major | Minor | Patch | Pre-release | Build metadata | None
Next version: x.y.z
Tag after merge: vx.y.z
Build metadata: N/A or +build.N
Rationale: Why this impact matches the public API or package behavior change.
```
