---
name: write-manual
description: Write or update the AYLO game developer manual (manual/pages) for anything a Luau script can see - calls, components, events, settings, setup forms, plugins - in a friendly, complete, example-driven style with checked snippets. Use whenever script-visible behaviour changes or a manual page is missing, wrong or thin.
---

# Write the game developer manual

The manual (`manual/pages/`, built with Zensical, published to GitHub Pages) is the **only** API reference a game
developer has: game work forbids reading C++. **If a script can see it, the manual explains it, in the same change.** A
feature with no manual page is not done (root `AGENTS.md`, rule 2: no manual change, no merge).

## When to run

Any change altering what a script can observe:
- an engine call added/renamed/given a new argument, result, default or error (`aylo.<plugin>.<verb>`);
- a component, resource or event: field, type, default, side;
- a setup form (`engine.assets`, `engine.input`...) or an `engine.*` helper;
- a `project.luau` setting or plugin name;
- a system phase, ordering name (`aylo.Physics.SyncBodies`) or a scripting-bridge rule (modules, sandbox, errors);
- a new plugin; networking or replication behaviour a game sees.

Pure internals need no manual change; say so in your summary.

## Before writing

1. **Read the truth**, not your memory: the plugin's `*Calls.hpp` (arguments, defaults, result), its components,
   `docs/<feature>.md`, and the script test in `tests/engine/`. Names, field types, defaults, units and sides must
   match exactly.
2. **Read the page you change and its neighbours** (`physics.md`, `engine-calls.md`, `components.md`, `modules.md` are
   the models) so voice, headings and tables stay consistent.
3. **Find the home**: an existing page, or a new page registered in `manual/mkdocs.yml` `nav` and linked from
   `index.md` and from pages that mention it.

## How to write

**Audience**: knows programming and a little Luau, never saw this engine, will copy your example. Luau only; never
mention C++, headers or classes of `src/`.

**Tone**: friendly and direct, second person, present tense, short sentences. Say what a thing is for before how it
works. No marketing, no filler, no "simply"/"just".

**Page shape** (drop a part only if it truly does not apply):
1. One-paragraph intro: what it gives a game and the smallest `project.luau` change to turn it on.
2. A **minimal working example**, early.
3. **Reference tables**: components (name, fields + types + defaults, side), events, calls (name, arguments with
   defaults, result), settings. Every row complete: no "etc.", no "see source".
4. **Recipes**: one per common task ("make a bullet", "play a sound on hit"), runnable as written, with the
   `phase`/`side`/`calls`/`query` it needs (a system that forgets to declare a call fails).
5. **Pitfalls and errors**: the exact message a script sees and its cause; which side has the feature; ordering rules;
   what the server owns and a client cannot write.
6. **See also**: relative links to related pages.

**Examples must be true.** Every snippet is Luau that runs, with real names, argument tables and phases. Take them from
or check them against `examples/games/aylo_duel/game/` and the script tests; run them when you can. An unchecked
example is worse than none - if you could not run one, say so.

**Consistency**: units and axes stated once and the same everywhere (world units, Y down, seconds); the side of every
datum named; engine names in code font (`aylo.Collider`, `aylo.physics.raycast`), first mention linked; at most one or
two admonitions per page; tables for facts, prose for reasons, code for how; relative links that resolve.

## After writing

1. Build: `uvx zensical build -f manual/mkdocs.yml` - no broken link or nav warning. (If `uvx` is unavailable, say so
   and check links by hand.)
2. Re-read each snippet against the code; `grep` every name you wrote.
3. Update the module's `docs/` design page when it states the same facts, so they do not drift.
4. Own commit: `docs(manual): ...` (`commit-conventions`); list changed pages in your summary.

## Done when

A newcomer can read only the manual, write a working script for the feature, and meet no name, default or behaviour
the page did not tell them. If you find a stale claim elsewhere (a doc saying something "is not implemented" that is),
fix it or list it in your summary (`plan-vs-implemented`).
