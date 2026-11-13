---
name: plan-tickets
description: Write well-formed tickets and split work into a dependency graph for the AYLO studio - titles, bodies with acceptance criteria, sizing, routing to the right agent, depends_on ordering. Use when triaging, splitting an epic, or creating a follow-up/bug/engine-gap ticket.
---

# Plan and write tickets

Tickets are GitHub issues on the project board. Maestro routes them to agents, runs one agent per PR-sized ticket and
respects `depends_on`. Good tickets are the difference between a clean PR and a clarification loop.

## Sizing: one ticket = one focused PR

- XS under an hour, S a few hours, M about a day. **L or bigger: split.** More than one speciality (design + engine +
  gameplay + docs) means split or a cell.
- Each sub-ticket is an independent PR that builds and passes on its own once its dependencies are merged.

## Routing table

| Work | Agent |
|---|---|
| C++ in `src/`, ECS, plugins, calls, rendering, network, physics, perf, xmake | `engine` |
| Luau game scripts, `project.luau`, assets, gameplay, UI, AI behaviours | `gamedev` |
| Mechanics, rules, balance, levels, economy; `docs/design/` and data files | `gamedesign` |
| `manual/`, `docs/` pages, examples, API reference | `docs` |
| CI, xmake/toolchain, release packaging, GitHub Actions, tooling | `devops` |
| Needs design -> engine API -> gameplay -> manual | split with dependencies, or a cell |

## Ordering rules (`depends_on`)

1. Design docs before the code implementing them.
2. Engine API (call/component/plugin) before the gameplay that uses it; its manual page lands with it (same ticket).
3. Data/config structure before the balance pass over it.
4. A migration ticket after the engine capability it adopts.
Never create cycles; give each sub-ticket explicit acceptance criteria.

## Title and body

Title: `[Area] <summary>` (`[Scripting] SCR-03: Component views` for a task of a plan document). Areas: ECS, Luau,
Scripting, Network, Physics, Vulkan, Audio, UI, Engine, Docs, Game, Design, CI.

Body (matches `.github/ISSUE_TEMPLATE/feature.yml` / `bug.yml`):

```markdown
## Summary
## Context          <why, links to design docs / manual pages / related tickets, file paths to touch>
## Usage            <for engine features: the Luau script that must work>
## Tasks            <checklist>
## Acceptance criteria   <testable bullets: what a reviewer can run or read>
## Technical notes  <constraints: side (server/client), replication, dependencies, risks>
## References
```
A bug states: steps to reproduce, observed, expected, platform, commit.

## Before creating

- `list_tickets` with a keyword (state any): update or comment on an existing one instead of duplicating.
- Name concrete files/modules (read-only look at the repo) so the assignee does not have to rediscover them.
- Out-of-scope discovery while working on your own ticket: `create_ticket` + mention in your summary. Do not do it.
- Engine gap from the game side: use `raise-engine-gap`. Engine update the game should adopt:
  `notify-game-on-engine-update`.

## Clarify only if blocked

Ask ONE precise question; propose your default so the answer can be "yes".

## Dependencies are real

Use `depends_on` (issue numbers); do not write "blocked by #x" as prose only - prose links nothing.
