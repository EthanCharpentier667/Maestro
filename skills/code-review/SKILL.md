---
name: code-review
description: Review checklist and verdict policy for pull requests on the AYLO engine and game repos - ticket acceptance, correctness, architecture rules, script API completeness, server authority, tests, docs, commit hygiene. Use when reviewing any PR diff.
---

# Review checklist

You are read-only (git diff/log/show, Read, Grep). You cannot run the build: judge from the diff, CI state you are
told, and the repo's own rules. Say what you could not check.

## Method

1. Read the ticket and every acceptance criterion. `git diff <base>...HEAD --stat`, then the full diff.
2. Read the AGENTS.md chain for the modules touched; the rules there are the review standard.
3. Walk the checklist below; classify each finding **blocking** or **suggestion**.
4. Verdict: approve unless there is at least one blocking item.

## 1. Ticket
Does it implement **every** acceptance criterion? Anything missing or only half done is blocking. Unrelated changes
(drive-by refactors, debug leftovers, build output) are blocking when large, suggestions when tiny.

## 2. Correctness
Edge cases (empty, zero, NaN, max), off-by-one, error paths, ownership/lifetime, iterator invalidation, deferred
`Commands` vs immediate use, integer overflow, concurrency/races across the server tick.

## 3. Architecture rules (engine) - each violation is blocking
- Dependency direction (contracts know no ECS, backends only their contract, plugins no backend/`luau`; static modules not linked by shared libs).
- Engine is not game: no gameplay or game names in `src/`.
- No global mutable state; third-party types not in public headers.
- A plugin owns its data, requires others instead of adding them, no cycle.
- Failures: returned vs thrown per module; **network/script/file input never crashes, throws unchecked, or asserts**.
- Portability: no compiler-specific code unguarded; paths and case.

## 4. The Luau API is the product
For any change a script can see (call, component, event, setting, setup form, plugin, bridge rule):
- is `manual/` updated and do the examples match the code? **No manual change = blocking.**
- is there a script test (`tests/engine/`, `tests/scripting/`) and a C++ test?
- is the new name stable and in the `aylo.` namespace? Is a breaking change flagged in the commit body and, if games are
  affected, is there a `[Engine] ... now available` ticket?
- argument fields have defaults; errors name the call.

## 5. Game (Luau) review
- Only Luau and `project.luau` changed; no C++.
- `require` by string (`@scripts/...`), entries named in `project.luau`, no globals used to share data, no legacy
  sorted-load pattern, nothing guarded by `engine.isServer` to skip a require.
- Components identical on both sides; server writes gameplay data; client only draws and sends input
  (`server-authority`).
- Every call/resource/event a system uses is declared; tunables are data, not magic numbers.
- Design doc values respected; deviations justified in the PR.
- An engine gap worked around by a hack instead of a ticket: blocking.

## 6. Tests and CI
Would CI pass on Linux/macOS/Windows? New behaviour covered; tests in their own commit; no disabled tests, no `NOLINT`
without reason. If you were told CI is red, that is blocking until fixed.

## 7. Security and robustness
Input validation (length, range, enum), unsafe deserialisation, path traversal outside `scripts/`/`assets/`,
secrets or tokens in the diff, `.env` committed, unbounded allocations from remote data.

## 8. Performance
Allocations or O(n^2) in per-tick loops, virtual calls in hot paths, blocking I/O on the game thread, per-frame string
building, queries re-created each tick. Blocking only when it is a clear regression on a hot path.

## 9. Commits and PR hygiene
Conventional Commits with bodies, lowercase summaries, atomic, tests separate, **no `Co-Authored-By`/AI attribution**,
no build artefacts, no secrets. These are suggestions unless CI enforces them (then blocking).

## Verdict policy
- **Blocking** = failures in 1-4/5/6/7 and clear regressions in 8. **Everything else is a suggestion.**
- Style nits that `clang-format`/`clang-tidy` handle are never blocking by hand.
- Each issue you return must be actionable: file:line, what is wrong, the minimal fix. No vague "consider refactoring".
- Be pragmatic: do not block on taste, naming preference or hypothetical future needs.
- Round 2+: only check that the previous blocking items were fixed and that the fix introduced nothing new.
