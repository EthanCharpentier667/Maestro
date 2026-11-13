---
name: notify-game-on-engine-update
description: Push side of engine/game sync - after you ship an engine change a game should act on (a lifted limitation, a new capability, a non-additive API change), open a ticket on the game repo assigned to the right agent. Use right before your final summary on any engine task that changes what Luau can observe.
---

# Notify the game on relevant engine updates

You are the engine agent and you have the freshest context on what you just shipped. Do not rely on the gameplay or
design agents to notice that a workaround they wrote is now unnecessary. This is the **push** side of staying in sync;
the `engine-update-watch` skill is the **pull** safety net the gameplay agent runs periodically. Push is faster.

## When to raise it

After your change is committed and verified (`verify` green), if it:

1. **Lifts a constraint the game side documented as a workaround.** Canonical case: `require`. It used to be a
   non-goal (`docs/scripting/plan.md` D6) and games shared state through sorted-load globals; D10 brought
   require-by-string (`@scripts/...`). Any similar lift - a missing setup form, an absent event, a hard-coded limit
   that became a setting, a field that was read-only - qualifies.
2. **Adds a capability that was never asked for** (the gameplay agent could not know to request it), or closes the
   loop on an engine-gap ticket in a way that needs game changes beyond "it works now" (for a plain gap ticket you
   already `comment_ticket` on it; this skill covers the *other* cases).
3. **Changes an API shape the game already codes against non-additively**: a renamed engine call, a changed component
   field type or default, a removed setting, a new required `calls = {...}` declaration, a stricter validation.
   This one is urgent: the game will break when it updates the engine.

Do **not** raise: internal refactors, perf work, build-system changes, new code nobody in Luau can see, or purely
additive details nobody was waiting for. This is a signal, not a changelog firehose - one noisy ticket trains the
gameplay agent to ignore the next one.

## How to raise it (Maestro)

1. Find the game repository of the project: look at `list_tickets` for tickets whose `repo` is not the engine, or at
   the engine's `examples/games/*` README. If you really cannot tell, say so in your summary instead of guessing.
2. Search first: `list_tickets` with a keyword - do not duplicate an existing "adopt X" ticket.
3. `create_ticket` with `repo` = the game repo, `assignee` = `gamedev`, and `helpers` = `["gamedesign"]` only when the
   update has a design angle (it changes scope, or opens a "should we now do X?" question). A pure drop-in
   simplification needs `gamedev` alone.
4. Title: `[Engine] <what shipped> now available - <action for the game>`.
5. Body - the ticket is the summary, **not a teaser**; nobody should have to read engine history to understand it:

```markdown
## What shipped
<one paragraph: the capability, in Luau terms, with the engine PR/commit/ticket reference>

## What it replaces or makes possible
<the workaround or limitation it lifts, with the file paths in the game that carry the workaround, if you know them>

## What the game should do
<concrete steps, ideally with the before/after Luau snippet>

## How to verify it
- Manual page: `manual/pages/<page>.md#<section>`
- Reference game: `examples/games/aylo_duel/game/scripts/<file>.luau`
- Test: `tests/engine/<ScriptXxxTests>.cpp`

## Compatibility
<breaking or additive; the engine version/commit it appears in; anything that now fails at load if left unchanged>

## Acceptance criteria
- [ ] ...
```

6. Mention the ticket in your final summary so the human sees the loop closed.

## What not to do

- Do not merge the engine change and leave the game-side workaround in place assuming someone will notice.
- Do not open a ticket that requires reading `Aylo-Engine`'s history to understand.
- Do not ask the game to adopt something you have not verified: re-run the relevant script test and cite it.
- Do not cc `gamedesign` on everything; only for design-relevant updates.
