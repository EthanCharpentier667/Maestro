---
name: raise-engine-gap
description: What to do when a game needs an engine feature that does not exist - prove it is missing, write the engine ticket with the Luau you would write, and keep working without hacks. Use whenever a game task is blocked by a missing call, component, event, setting, plugin or manual page.
---

# Raise an engine gap

The game side never edits C++ and never invents an API. When the engine lacks something, you file a precise ticket for
the `engine` agent and continue with what is possible. This is a core loop of the studio, so the ticket must be good
enough that the engine agent can implement it without asking you anything.

## 1. Prove it is really missing

Run `engine-update-watch` / `engine-repo-sync` first, then:

- search the manual (`manual/pages/`) and the setup/call tables for the capability;
- grep the docs (`docs/ecs/features/19-luau-bridge.md`, `docs/scripting/plan.md`) and the reference game scripts
  (`examples/games/aylo_duel/game/scripts/`) for the likely call or component name - you never read C++ for game work;
- `list_tickets` with a keyword: it may already be filed, planned, or in progress. If so, `comment_ticket` with your use
  case instead of duplicating.

If it exists but is undocumented or the manual is wrong, that is a docs fix (`write-manual`), not an engine ticket.

## 2. Check whether it is a *non-goal* or an *undecided design*

`docs/scripting/plan.md` records decisions (D1..). A non-goal that was later reversed (e.g. D10 for `require`) is
available. A true non-goal needs a conversation, not a ticket: explain the sanctioned pattern.

## 3. File the ticket (`create_ticket`)

- `repo`: the **engine** repository (pass it explicitly; the default is the repo of your current ticket).
- `assignee`: `engine`. Add `depends_on` only for real ordering.
- Title: `[Area] <summary>` with Area one of ECS, Luau, Scripting, Network, Physics, Vulkan, Audio, UI, Engine, Docs
  (e.g. `[Physics] expose raycast filters to scripts`).
- Body, following the engine's feature template:

```markdown
## Summary
<one sentence: what a script cannot do today>

## Context
<which game task is blocked, which ticket of the game repo; what you tried; the manual page or doc that confirms the gap>

## Usage
<the Luau you WANT to write, complete and runnable if the feature existed: the engine.call / component / setup form /
project.luau setting, with the phase, side and `calls` declaration>

```luau
engine.ecs.system("game.Aim", { phase = "FixedUpdate", calls = { "aylo.physics.raycast" }, ... })
```

## Acceptance criteria
- [ ] a script can <do X> on <server|client|both>
- [ ] the manual page <name> documents it
- [ ] a script test covers it

## Technical notes
<constraints you know: replication, side, determinism, performance. Do not design the C++.>
```

## 4. Keep working

- Block your own ticket only if nothing useful can be delivered. Prefer delivering the parts that do not need the gap
  and say clearly in your summary what is blocked and by which ticket number.
- Do **not** ship a hack that fakes the engine feature (parsing files by hand, polling hacks, a workaround that will
  fight the real API later). A temporary stand-in is acceptable only if isolated in one module with a comment
  `-- TODO(engine #n): replace with <call>` and mentioned in the summary.
- When the engine ticket lands, the engine agent notifies you (`notify-game-on-engine-update`) and you adopt it
  (`engine-update-watch`).
