---
name: commit-conventions
description: Commit message and branch hygiene rules. Use whenever you are about to commit.
---

# Commit conventions

- Conventional commits: `feat(scope): …`, `fix(scope): …`, `refactor`, `docs`, `test`, `chore`, `perf`.
- Subject ≤ 72 chars, imperative mood; body explains *why*.
- One logical change per commit; never commit build outputs, secrets, `.env`, or large binaries.
- Run the formatter/linter of the project before committing if one is configured.
