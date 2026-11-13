---
name: engine-update-watch
description: Pull side of engine/game sync - at the start of a game task or milestone, check what the engine shipped since last time and adopt any capability that removes a workaround in the game. Use when starting a new gameplay task, at milestone boundaries, and whenever you are about to write a workaround for an engine limitation.
---

# Engine update watch - keep the game current

The AYLO engine evolves in parallel with the game. A workaround you wrote against last week's engine can be dead
weight today, and nobody will tell you unless you look. Do not wait for a missing feature to force you to read the
engine; spend two minutes at the start of each task.

## Check (2 minutes)

1. Locate the engine checkout available to you (the engine repo is `Aylo-Engine`; the game's CI or
   `docs`/README names the version it targets). If none is available, read the manual pages you rely on on the engine
   repo's default branch instead.
2. `git fetch origin && git log --oneline <last-known-sha>..origin/main` (or the last 30 commits). Skim titles; open
   anything touching `manual/`, `docs/scripting/`, `docs/ecs/features/`, `docs/engine/`, a plugin you use.
3. Re-read the manual pages for the features the game uses (`manual/pages/modules.md`, `systems.md`, `components.md`,
   `engine-calls.md`, `setup-forms.md`, `project-settings.md`, `networking.md`, `physics.md`, ...). Look for:
   status language that changed ("not available yet" gone), a new section, a changed default.
4. Ask the one question that matters: **does anything newly shipped let me delete a workaround or simplify code I
   already wrote?** That is the signal worth acting on, not "is there a new feature in the abstract".

Also check `list_tickets` for `[Engine] ... now available` tickets created by the engine agent
(`notify-game-on-engine-update`) - those are the engine telling you directly.

## Canonical example: `require`

`require` was once a non-goal (`docs/scripting/plan.md` D6): games shared code through a global table set in a file
that sorted before its consumers. D10 (epic #489) introduced real require-by-string. If the game still has the old
pattern (a file setting `Balance = {...}` as a global, consumers reading the bare name, files named to control sort
order), then:

- Migrate those files to `local balance = require("@scripts/shared/balance")` with `return { ... }` modules
  (see `luau-modules`), name the entries in `project.luau`, and update comments/tests that documented the old
  constraint.
- Do it in the same ticket if it is small, otherwise `create_ticket` a focused migration ticket.
- Treat it as the template: **any time a limitation you designed around is lifted, the default action is a change that
  adopts the new capability, not a backlog note.**

## Before adopting

- Re-confirm the capability actually works: the manual page, a test in `tests/engine/`, and the reference game
  (`examples/games/aylo_duel/game/`). Verify good news as hard as bad news.
- Keep the change scoped to adopting it (migration + the tests it touches). No drive-by refactor.
- If adopting needs an engine-side change that is half-landed or missing, it is an engine gap: `create_ticket` for the
  `engine` agent, with the Luau you would write. Do not hack around it again.

## Record what you looked at

In your final summary: "Checked engine up to <sha/date>; adopted X; nothing else relevant." This is what the next run
(and the reviewer) uses as its `<last-known-sha>`.
