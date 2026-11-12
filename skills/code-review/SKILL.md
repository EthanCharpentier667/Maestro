---
name: code-review
description: Checklist for reviewing a pull request. Use when reviewing changes.
---

# Review checklist

1. Does it implement every acceptance criterion of the ticket?
2. Correctness: edge cases, null/empty, off-by-one, error paths, concurrency.
3. Build & tests: would CI pass? new code covered where reasonable?
4. Security: input validation, secrets, unsafe deserialisation, injection.
5. Performance: allocations or O(n²) in hot paths, blocking I/O on main/game thread.
6. Scope: no unrelated changes, no debug leftovers.
Blocking = 1-5 failures. Everything else is a suggestion.
