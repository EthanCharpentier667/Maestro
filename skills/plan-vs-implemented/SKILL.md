---
name: plan-vs-implemented
description: Tell shipped-behaviour docs from design/plan docs in the AYLO repos, verify "is X implemented" against src/, tests/ and the reference game, and decide between implementing, asking, or explaining a settled non-goal. Use whenever a question or ticket depends on whether the engine can do something.
---

# Plan vs implemented - doc triage and close-the-gap

`docs/` mixes two kinds of document, and conflating them is the most common way engine questions get answered wrong.

- **Shipped behaviour docs** describe what the code does today: `docs/Modules.md`, `docs/Plugins.md`, `manual/pages/*`.
- **Design / plan docs** describe a decided or undecided future: `docs/scripting/plan.md`,
  `docs/ecs/features/*`, `docs/network/implementation.md`, `docs/release/plan.md`, `docs/study/*`.

Red flags that a doc is aspirational: "plan", "proposal", "RFC", "roadmap", "study" in the path or title; phrases like
"for now", "a later revision", "non-goal", "target of the first version"; a dated decision log (D1, D2, ...); snippets
introduced as "a recommendation, not an obligation". **Read a decision log to the end - later rows supersede earlier
ones** (example: `docs/scripting/plan.md` D6 said `require` was a non-goal; D10 reversed it and the manual's
`modules.md` documents it).

## Never answer "is X implemented" from a doc alone

1. **Source.** `grep -rn` the concrete thing in `src/`: a registered call name (`aylo.audio.loadSound`), a Luau global,
   a class, a component name. Wired up in `src/`? If not found it is a plan, however confident the prose.
2. **Test.** Is there a test exercising it? C++ in `tests/<module>/`, script tests in `tests/engine/` and
   `tests/scripting/`. No source and no test = not implemented.
3. **Reference game.** `examples/games/aylo_duel/game/scripts/` is real Luau on this exact engine. When doc and game
   disagree, trust the game and flag the doc as stale.
4. **Manual.** `manual/pages/` is what game developers are told; a feature in code but absent from the manual is
   *unfinished* (rule 2), not missing.

Always run `engine-repo-sync` first so you read the current tree.

## When you find a real gap

Classify it, then act - never stop at "not implemented, here is why".

| Situation | Action |
|---|---|
| **Design decided** (the plan states a concrete shape, not an open question) **and something is blocked on it** | Implement it: code + tests + docs, mark the decision shipped in the plan doc (remove "non-goal"/"for now" once false), add the manual page. Comment on the blocking ticket (`comment_ticket`) saying what shipped and where. |
| **Design undecided** (open question, or no doc covers the shape) | Do **not** ship a guess: a game will hard-code against whatever you ship. Comment with 2-3 concrete options and a recommendation; ask one precise question; if you cannot continue, say so in your summary so the ticket goes to a human. |
| **Decided and intentionally out of scope** | Not a gap. Explain the sanctioned pattern (usually visible in the reference game) instead of re-litigating. Double-check there is no later decision reversing it. |

## Self-check before replying to "can the engine do X?"

- Did I grep `src/`, not just read a doc?
- Did I look at `tests/` and the reference game?
- Did I read the whole decision log, including the newest row?
- If it is missing and decided: did I implement it (or file the ticket with `create_ticket`) rather than only report it?
- If it is undecided: did I ask instead of guessing?

## Reporting

State the evidence in your answer: file paths and names you found (or searched without result), the test, the decision
id. "Not implemented" without a grep you can cite is not an answer.
