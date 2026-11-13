---
name: engine-repo-sync
description: Re-sync the engine checkout with origin/main before investigating, implementing, answering "does X exist in the engine", and again right before finishing. Use at the start of every engine task and whenever you rely on memory of what the engine contains.
---

# Engine repo freshness sync

The engine (`Aylo-Engine`, the repo on this ticket) is developed continuously, by several agents across many separate
runs, and the game repo reads its `docs/`, `manual/` and behaviour from a different checkout. A stale worktree makes you
confidently restate a limitation that was fixed last week, or re-implement something that already shipped. A ten-second
fetch avoids both. **Never answer "does X exist / is Y implemented" from memory of a previous run.**

## At the start of the task

Maestro created your worktree from the base branch, but it may be behind. Always:

```bash
git status --short                      # what is already modified here (should be nothing but your work)
git fetch origin
git log --oneline HEAD..origin/main     # what landed since this worktree was cut
git diff --stat HEAD...origin/main      # which areas moved
```

- Nothing new: carry on.
- New commits and your branch has none of its own yet: `git merge --ff-only origin/main`.
- New commits and you already committed: `git rebase origin/main` (you own this unpushed branch, so rebasing is safe).
  Resolve conflicts by reading both sides - never take "ours" blindly and never drop the other task's change.
- Read the commit titles touching the module you work on (`git log --oneline HEAD..origin/main -- src/aylo/<module> docs manual`).

## Verify a claim against the synced tree

When a ticket, comment or doc says "the HTTP plugin does X", "require is not implemented", "the editor can Y":

1. `grep -rn` the concrete name in `src/` (a call `aylo.http.request`, a class, a component).
2. Look for a test in `tests/` exercising it (`tests/engine/` for script tests).
3. Look at the reference game `examples/games/aylo_duel/game/scripts/` - it is real Luau that runs today.
4. Read the matching `manual/pages/*.md`. If manual, code and test disagree, trust code + test and fix the doc.

Use `plan-vs-implemented` to classify docs; this skill only guarantees you are looking at the current tree.

## Before you finish (second sync)

A long investigation can span enough time that `main` moved. Immediately before your final commit and summary:

```bash
git fetch origin && git log --oneline HEAD..origin/main
git rebase origin/main        # if anything landed; then re-run the cheapest relevant tests
```

If the rebase touched files you edited, rebuild/retest (`verify`) before declaring done, and mention the rebase in the
summary. Maestro pushes for you, but a silent merge conflict or reintroducing something another task just removed is
your responsibility, not the reviewer's.

## Typical mistakes this prevents

- Reporting "`require` is a non-goal (D6)" when D10 reversed it - read the *decision log* to the end.
- Documenting an engine call under its old name after a rename on `main`.
- Opening a ticket for a gap another run already closed (search `list_tickets` too).
