---
name: luau-modules
description: How script files share code, data and config in AYLO games - require-by-string, entry points, frozen module tables, per-file globals, cycles, caching, and the legacy sorted-global pattern to remove. Use before writing any .luau file that needs something from another file, or when a script "does nothing" because it never loaded.
---

# AYLO Luau modules (`require`)

Authoritative page: `manual/pages/modules.md` in the engine repo. Decision record: `docs/scripting/plan.md` **D10**
(epic #489). If the engine you target predates D10, check `engine-update-watch` first; do not assume.

> **History trap.** D6 said "`require` is a non-goal, files load in sorted path order and share plain globals". D10
> *fully supersedes* it. Old game code, old skills and old PR comments still describe the sorted-global workaround.
> That pattern is **wrong now**: files no longer auto-load, and each file has its own globals.

## The model

- **Nothing under `scripts/` runs by itself.** Only the *entry points* named in `project.luau` run:
  ```luau
  engine.settings.scripts.main   = "shared/main"   -- both sides, first
  engine.settings.scripts.server = "server/main"   -- servers (and standalone)
  engine.settings.scripts.client = "client/main"   -- clients (and standalone)
  ```
  With none set, the entry is `scripts/main.luau`. A file that no entry requires (directly or not) never runs - and
  **nothing warns you**: its components and systems are just missing.
- **Each file has its own globals.** `math`, `string`, `table`... and `engine` are the same for all but **read-only**
  (`engine.assets.x = 1` is an error). A global you create (`Counter = 1`) is visible to your file only. Never use a
  global to pass data.
- **Share through `require` and `return`.**

```luau
-- scripts/shared/balance.luau
return { tickSeconds = 0.1, shipSpeed = 4 }
```
```luau
-- scripts/server/ships.luau
local balance = require("@scripts/shared/balance")
engine.ecs.system("game.MoveShips", { ... run = function(ctx) ... balance.shipSpeed ... end })
```

## Path forms

No extension, `/` only. Resolves to `x.luau` or `x/init.luau` (both existing is an error).

| Form | Meaning |
|---|---|
| `"./x"` | next to the requiring file |
| `"../x"` | folder above (never above `scripts/`) |
| `"@scripts/x"` | from the root of `scripts/`, identical from every file - **prefer this in games** |
| `"@self/x"` | inside the folder of the requiring file (in `init.luau`: inside its own folder) |

Extra aliases can come from settings (`scripts`/`self` are reserved). Case must match the real file name exactly (so a
game written on macOS/Windows also loads on Linux). Nothing outside `scripts/` is reachable.

## Rules that bite

1. **A module returns nothing or exactly one value.** Two or more is an error.
2. **A returned table is frozen (shallow).** Reading works, writing a field raises. Mutable state lives in a `local`
   behind functions:
   ```luau
   local score = 0
   return { add = function(n) score += n end, get = function() return score end }
   ```
3. **A module runs once**, on first `require`; later requires return the same value (`./a` and `@scripts/a` share one
   instance).
4. **`require` at the top of the file, never inside `run`.** Systems run on the schedule; they may only require modules
   already loaded.
5. **Components/systems belong to the module that declared them.** Declare identical components on both sides in a
   shared module that both entries require (the network manifest checks they match).
6. **Cycles** error with the chain (`cycle: a -> b -> a`): move what both need into a third module.
7. **Never `engine.isServer`/`isClient`-guard a require to skip a file.** Put client-only code under the client entry
   (`client/main.luau`); a client-only plugin does not exist on the server and the file would fail there.
8. **`dofile`, `loadstring`, `load` do not exist.** Do not try to emulate them.
9. A failed module is not cached: fix and reload. Errors read `<file>:<line>: require("<path>"): <reason>`
   (`module not found`, `ambiguous`, `escapes scripts/`, `case mismatch`, `unknown alias`, `cycle`, `returned N values`).
10. Every game folder carries a `.luaurc` next to `project.luau` for the editor (aliases); keep it in sync with the
    aliases you add.

## Recommended layout (mirror `examples/games/aylo_duel/game/scripts/`)

```
scripts/
  shared/   main.luau (entry), rules.luau, arena.luau   -- declarations both sides need, balance tables
  server/   main.luau (entry), players.luau, weapons.luau ...
  client/   main.luau (entry), sprites.luau, hud.luau ...
```
`shared/main` requires shared modules; `server/main` requires every server file; same for the client. A new file = a
new `require` line in the entry of its side.

## Removing the legacy pattern (when you inherit it)

Symptoms: a file doing `Balance = {...}` without `local`/`return`; consumers using the bare name `Balance.x`; files
renamed `00_config.luau` to control load order; comments saying "require is not available"; tests that "pass" under a
standalone `luau` interpreter but fail in `aylo-runtime`.

Migration: (1) turn each global-setting file into a `return { ... }` module; (2) add
`local x = require("@scripts/...")` at the top of each consumer; (3) name entries in `project.luau`; (4) delete the
ordering-by-filename and the outdated comments; (5) run a server and a client and check the log for load errors.
Tests must run on `aylo-runtime`/the engine's script-test harness, not a bare `luau` binary whose `require` semantics
differ.
