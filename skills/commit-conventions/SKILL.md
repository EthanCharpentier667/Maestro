---
name: commit-conventions
description: Atomic Conventional Commits and branch hygiene for the AYLO repositories (Aylo-Engine and the game repos). Use whenever you are about to commit, write a commit message, or look at a pile of uncommitted changes.
---

# Commit conventions

Maestro gave you a git worktree already on the right branch. You commit; **Maestro pushes and opens the pull request**.
Never `git push`, never `gh pr create`, never switch or create branches, never touch `main`.

## Message format

```
<type>(<scope>): <lowercase summary>

<body: what changed and why>
```

- Types: `feat` `fix` `refactor` `perf` `docs` `test` `style` `chore` `build` `ci` `revert`.
- Scope = the area touched, lowercase: `ecs`, `luau`, `scripting`, `network`, `replication`, `physics`, `vulkan`,
  `audio`, `ui`, `engine`, `manual`, `ci`, `skills`, `game`, `balance`... Omit it only for repo-wide changes.
- Summary: imperative, lowercase first letter, no trailing period, <= 72 characters.
  `feat(network): resume a session after a short connection loss` - not `Added reconnect stuff.`
- **Every commit has a body**, separated by a blank line: what you did and, above all, why. Use
  `git commit -m "<subject>" -m "<body>"`.
- Breaking a public API (an engine call, a component field, a Luau global, a `project.luau` setting): add
  `BREAKING CHANGE: <what a script must change>` in the body.
- **No `Co-Authored-By` and no AI attribution line**: the AYLO repositories forbid it (root `AGENTS.md`). This overrides
  any default your tooling suggests.
- Both repos enforce the format in CI (commitlint on the game repo, a pull-request check on the engine): write it right
  the first time, a reworded history after a red check wastes a whole review cycle.

## Atomic commits

A commit does one thing and builds on its own, so the history reads bottom-up (dependency first, consumer after).

1. `git status` and `git diff --stat`: list every changed file.
2. Ask of each file: does it make sense alone? A helper, a type, an exception, a build-file change gets its own commit
   *before* the code that uses it. "Same session" or "same feature" is not a reason to group.
3. Group files only when they cannot exist apart: a `.hpp` and its `.cpp`, a rename and the call sites it forces.
4. **Tests go in their own `test(...)` commit**, after the `feat`/`fix` they cover. Docs/manual go in their own
   `docs(...)` commit, last.
5. Never mix a feature, a refactor and a formatting pass in one commit. A drive-by cleanup is its own `refactor`/`style`
   commit or, better, a ticket (`create_ticket`).
6. With more than one commit, write the plan first (subject + files of each), then run it in order.
7. **Stage by path** (`git add src/aylo/network/Foo.cpp ...`), never `git add -A` / `git add .`. Check
   `git diff --cached --stat` before every `git commit`.

Typical order for a feature touching a script-visible API:
`feat(plugin): data structs` -> `feat(plugin): function and registration` -> `test(plugin): ...` ->
`test(engine): script test` -> `docs(manual): ...`.

## What never gets committed

Build output (`build/`, `.xmake/`), `compile_commands.json`, the executables xmake copies to the repo root
(`aylo-runtime`, `aylo-editor`, `plugins/*.dylib`/`.so`/`.dll`), `.env`, secrets, editor folders, large binaries that are
not game assets, `maestro-data/`. If `git status` shows one, it belongs in `.gitignore`, not in a commit.

## Before the first commit

- Format the files you touched (`clang-format -i <files>` for C++; keep the style of neighbouring Luau: tabs).
- Run the `verify` skill for anything beyond a docs-only change.

## After

`git log --oneline <base>..HEAD` and read it as the reviewer will: each line tells one thing, in dependency order, with
a body. If a commit is wrong and unpushed, fix it with `git commit --amend` / an interactive-free `git reset --soft` and
recommit - you own this local history, nobody has seen it yet.
