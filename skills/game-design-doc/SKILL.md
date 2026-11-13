---
name: game-design-doc
description: Template and rules for AYLO game design documents (mechanics, systems, levels) written so a Luau programmer can implement them without guessing - intent, rules, ECS data model, sides, tuning table, edge cases, acceptance criteria. Use when creating or editing anything in docs/design/.
---

# Game design doc format

File: `docs/design/<system-or-feature>.md` - one doc per system, cross-linked. A design doc is a contract with the
gameplay programmer (`gamedev`) and the reviewer: if a number or a rule is not in it, it does not exist.

```markdown
# <Feature name>
Status: draft | agreed | implemented   ·   Owner: gamedesign   ·   Related: [other-doc](other-doc.md), ticket #n

## Intent             the player fantasy / feeling in 1-3 sentences; what decision the player makes
## Rules              numbered, unambiguous, testable ("R1. A ship loses 1 hp per laser hit")
## Data model         the ECS shape (see below)
## Controls & feedback  inputs (action names), VFX / SFX / UI feedback, timing of each
## Tuning             table: parameter | value | unit | range | notes   (see game-balancing)
## Edge cases         what happens when... (simultaneous hits, disconnect, 0 players, tie, pause)
## Acceptance criteria  testable bullets a programmer and a reviewer can check
## Open questions     each with an owner and a proposed default
```

## Data model section (this is what makes the doc implementable on AYLO)

For each piece of state say, in design terms the programmer can map directly:
- the **component** (`game.Ship { hp: f32, player: u32 }`), its fields, types and defaults;
- the **side that owns it** (server / client / both) and whether it must be **replicated** (every change / at spawn /
  never) - gameplay truth is always server-side, presentation client-side (`server-authority`);
- the **inputs** (action names, type, bindings) clients send;
- **events / messages** (a moment, not state);
- **tuning keys** that live in a shared balance module (name them: `balance.laser.damage`).
You do not write code, but you do use the engine's vocabulary. Check `manual/pages/` for what exists; if your design
needs something the engine lacks, list it under Open questions as **Engine need** with the Luau you imagine, so
`gamedev` can `raise-engine-gap`.

## Writing rules

- **Concrete numbers** in Tuning, never "fast", "a lot", "soon". Include units (world units, seconds, per second) and a
  sane min/max.
- **Rules are atomic and numbered**; acceptance criteria reference rule numbers (`AC3 covers R4, R5`).
- Describe the **time axis**: what happens on tick N vs per frame; durations in seconds, not frames, unless it is a
  fixed-step rule.
- Cover **multiplayer**: what each player sees, what happens on join/leave mid-round, latency tolerance, anti-cheat
  implication (the server decides).
- State **what is out of scope** explicitly, so the programmer does not add it.
- Keep one doc per system; link rather than duplicate numbers (a number lives in one table).
- When changing an agreed doc, update `Status`, add a changelog line at the bottom, and tell the implementers via a
  ticket comment so code and doc do not drift.
- Data files you may edit (balance modules as text, JSON/YAML): keep their keys identical to the Tuning table.

## Checklist before you finish

- [ ] A programmer can implement every rule without asking you a question.
- [ ] Every number is in a Tuning table with unit and range.
- [ ] Sides and replication stated for each state.
- [ ] Edge cases include disconnect, tie, zero/one player, and simultaneity.
- [ ] Acceptance criteria are testable (observable values or log lines, not "feels good").
- [ ] Open questions carry a default so work is not blocked.
