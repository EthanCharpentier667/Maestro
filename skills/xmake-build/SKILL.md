---
name: xmake-build
description: Build, test, lint, package and CI conventions of the AYLO engine - xmake commands, target layout, plugin libraries, GitHub Actions gate, stacked PR behaviour, release zip. Use for any build-system, CI, packaging or tooling change, or to diagnose a build failure.
---

# xmake, CI and release for AYLO

## Commands (clang toolchain, C++23)

```sh
xmake f -y [-m release] [--Tests=y] [--aylo_duel=y] [--AllExamples=y] [--nightly=<sha>]
xmake build -y                  # everything
xmake build engine              # one target
xmake test -v [<module>-tests/*]
xmake project -k compile_commands          # needed by clang-tidy
xmake run aylo_duel server [--port N] / client [--host H --port N]
.github/scripts/lint.sh <base>             # clang-format + clang-tidy on changed files
uvx zensical build -f manual/mkdocs.yml    # the manual site
```
Windows uses the MSVC toolchain; elsewhere clang. Examples are opt-in options (`--BasicGLFWWindow=y`, ...).

## Layout of what is built (docs/release/package.md)

`aylo-runtime` (executables and tests at the folder root), `lib/` (shared libraries that are not plugins: `ecs`,
contracts, `engine`...), `plugins/` (plugin libraries, real files, scanned by the engine), backends under their own
folder. The same layout in the build folder and in the release zip. The build copies `aylo-runtime` to the repo root:
**never commit those copies**.

## Rules

- `set_version` lives in the root `xmake.lua` only (`aylo-runtime --version` and the release package read it).
- Sources are found by glob in each module's `xmake.lua`: a new `.cpp` needs no build-file change; a new dependency goes
  through `add_requires` in the module (root file if several modules use it) and stays private.
- Static modules (`luau`, `network`, `replication`, `scripting`, `engine`) must not be linked by shared libraries - rule
  `aylo.static_modules` refuses it. Do not "fix" a link error by weakening the rule; fix the dependency direction.
- Plugins are declared with `plugin("<name>", {<deps>})` (applies `aylo.plugin_library`); backends have their own helper.
- Pin versions of packages and Actions (`vulkan-headers 1.4.x`, `glfw 3.4`, action SHAs/tags). Never use `latest`.
- Reproducible, cached, fail fast. `XMAKE_ACTION_BUILD_CACHE=true` enables the build cache for all compilers in CI.

## CI (.github/workflows/ci.yml)

- Builds and tests on Linux, macOS, Windows; lints the changed files with the same `lint.sh`.
- **Stacks** (`gh stack`): only the top PR of a stack runs the checks, on the whole stack; the others skip them via the
  `gate` job. A PR retargeted to `main` after the one below merged must be re-run by hand (`gh run rerun` /
  `workflow_dispatch`).
- The concurrency group cancels in-progress runs of the same ref.
- Permissions are read-only by default; add the minimum a job needs, never `write-all`.
- Other workflows: `manual.yml` (publish the manual to Pages), `release.yml`, `cache-cleanup.yml`, `issue-area.yml`.
- Never commit secrets; use repository/environment secrets and list required ones in your summary.

## Diagnosing a red build

1. `gh run list --branch <b> -L 5`, `gh run view <id> --log-failed`: read the first real error, not the summary.
2. Reproduce with the same command and the same toolchain flags (`-m release` vs debug, `--Tests=y`).
3. Platform-only failure: check includes (`<cstddef>` for `offsetof`), `std` features missing in Apple Clang/MSVC,
   case-sensitive paths, `/` vs `\`, exported symbols (`AYLO_*_API`) on Windows.
4. Package failure: a pinned version vanished or a system package shadows `system = false`.
5. Flaky or infra: say which check and why you think so; do not retry blindly in a loop.

## Releases

Milestones are tagged on `main` (`v1.0.0`...); `0.x` until the script API settles (`docs/release/plan.md`,
`release-process.md`). `xmake package-release` builds the zip game developers download (signing: `docs/release/signing.md`).
Do not tag or publish a release yourself - open a ticket describing it.
