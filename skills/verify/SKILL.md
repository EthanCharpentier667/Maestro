---
name: verify
description: Verify a change against the AYLO definition of done and the CI gate (build, tests, format/lint, docs, script test) and report each step's real outcome. Use before your final summary on any code, docs or script change, and whenever you are about to say "done".
---

# Verify a change (definition of done + CI gate)

Maestro opens the pull request from your branch and CI (Linux, macOS, Windows) runs the same checks. A PR that goes to
review with red CI wastes a human's time on what you could have caught. Run these from the repo root, in order, and
**report what really happened**. Your final summary becomes the PR description.

## Engine repo

### 1. Configure and build, with tests
```sh
xmake f -y --Tests=y        # + --aylo_duel=y or --AllExamples=y if you touched an example
xmake build -y
```
Skip only if you touched no C++ (docs, Luau, `.github`) - and say you skipped.

### 2. Test
```sh
xmake test -v                    # -v is mandatory: without it you cannot see what failed
xmake test -v engine-tests/*     # one module: <module>-tests/*
```
New behaviour has a test in `tests/<module>/`. A script-visible feature also has a script test (`tests/engine/`,
`tests/scripting/`).

### 3. Format and lint the changed files
```sh
xmake project -k compile_commands
.github/scripts/lint.sh origin/main      # base = the base of your branch
```
`lint.sh` runs `clang-format` and `clang-tidy` over every C/C++ file changed since the base, in full, like CI.

### 4. Docs
`git diff --stat origin/main`: if the change touches an engine call, component, event, setting, setup form or plugin
and `manual/` is not in the diff, stop and run `write-manual`. The `docs/` page of the module is updated. Paths your
docs/AGENTS.md/skills mention exist.

### 5. Commits
`git log --oneline origin/main..HEAD`: Conventional Commits, lowercase, a body each, tests in their own commits, no
AI attribution (`commit-conventions`).

## Game repo (Luau)
- Run the game: server + client with the generic executables; read the log for system/require errors.
- Run the repo's script tests if any (they must run on the engine, not a standalone interpreter - see `luau-modules`).
- Commitlint format on messages; no committed build artefacts.

## CI failure loop (when feedback says CI is red)
1. Read the failing job's real log, not the summary: `gh run list --branch <branch> -L 3`, then
   `gh run view <id> --log-failed`.
2. Reproduce locally with the same command (`xmake test -v`, `lint.sh`, `commitlint`).
3. Fix the root cause (a failing test, a lint violation, a platform compile error), recommit. Never disable a test or add
   `NOLINT` to silence a real finding.
4. A failure not yours (infra flake, missing secret): say exactly which check, why you believe so, and stop.

## Report faithfully
- List each step: **pass / fail / not run (reason)**: missing tool, no GPU, platform-specific.
- Never write "builds" or "tests pass" for a step you did not run. A failing step is reported with its first error; a
  pre-existing failure is named as such.
- Say what only CI can check (macOS, Windows, MSVC).
- Template for your final summary: **Summary** / **Why** (include assumptions) / **Changes by module** /
  **Test plan** (checklist with real results) / **Follow-ups** (tickets you created).
