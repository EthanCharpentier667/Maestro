---
name: luau-gameplay
description: Write or change an AYLO game in Luau - project.luau, components, systems, engine calls, setup forms, inputs, assets - without touching C++. Use for any work in a game/ folder or any ticket about gameplay, controls, AI, UI or game rules.
---

# Write a game in Luau (AYLO)

A game is a folder; the engine runs it with generic executables. **You write Luau and `project.luau` only.** Never read
or change C++ (`src/`, a game's own `main`). The C++ parts of `aylo_duel` / `ethan_game` are a temporary step, not a
model. Learn the API from `manual/pages/` in the engine repo (the game developer manual); where it is silent,
`docs/scripting/plan.md`, `docs/ecs/features/19-luau-bridge.md`, `docs/engine/design.md`, `docs/luau/type-definitions.md`.
**If none of them says an API exists, it does not**: file an engine ticket, do not work around it
(`raise-engine-gap`).

Related: `luau-modules` (require), `server-authority` (who owns what), `engine-update-watch`, `verify`.

## Folder layout

```
my_game/game/
  project.luau      settings, run before anything (required)
  .luaurc           editor aliases
  scripts/          entries named in project.luau + the files they require
  assets/           textures, sounds, fonts, scenes, actors - named by path under assets/
```
Run: `./aylo-runtime --server my_game/game` and `./aylo-runtime my_game/game`. Each `--name value` flag is
`engine.args.name` in `project.luau`. Model on `examples/games/aylo_duel/game/` (`scripts/shared|server|client`).

## project.luau

Plain Luau setting `engine.settings`; `engine.isClient`, `engine.isServer`, `engine.isDebug` say where it runs.
Choose everything here: `network.port/host`, `plugins` (names like `"aylo.Physics"`; clients also `"aylo.graphics"`,
`"aylo.Sprite"`, `"aylo.audio"`...), their sections (`physics.gravity`), and on clients `platform`, `renderer`,
`window.title/size`, `audio.*`, `scripts.main/server/client`. **Unknown keys are errors** - a typo fails at start. Never
change engine code to change a setting.

## Components

```luau
engine.ecs.component("game.Ship", {
	fields = { position = "vec2", player = "u32", hp = "f32" },
	defaults = { hp = 100 },
	replication = "everyChange",    -- "none" | "everyChange" | "spawnOnly"
})
engine.ecs.component("game.Timer", { fields = { remaining = "f32" }, presence = "serverOnly" })
```
Field types: `bool`, `i8..i64`, `u8..u64`, `f32`, `f64`, `entity`, `vec2`, `vec3`, `vec4`, `assetId`, `string` (<= 255
bytes). `vec3` reads as a Luau `vector`; `vec2`/`vec4` read and write as tables with **every key present**. Declare the
same components on both sides in a shared module. `presence`: `serverOnly`, `clientOnly`, or both. Namespace yours
`game.` (never `aylo.`).

## Systems

```luau
engine.ecs.system("game.MoveShips", {
	phase = "FixedUpdate",   -- Startup | PreUpdate | FixedUpdate | Update | PostUpdate | Render
	side = "server",         -- "server" (default) | "client" | "both"
	query = { read = { "game.Ship" }, write = {}, with = {}, without = {} },   -- required, {} for none
	resources = {}, events = {}, calls = { "aylo.replication.readInput" },
	after = { "aylo.movement" },
	run = function(ctx)
		for entity, ship in ctx.query do ... end
	end,
})
```
- `ctx` has `query`, `commands` (spawn, add, patch, setParent, removeParent, despawn - **deferred**), `time`
  (`ctx.fixedDelta`), `resources`, `events`, `call`.
- **Declare every call, resource and event you use**; an undeclared `ctx.call` fails.
- A component view is valid only inside the `run` that received it - never store it; storing an `Entity` handle across
  ticks is fine.
- Use `FixedUpdate` for simulation (deterministic step), `Update` for per-frame presentation, `Render` for drawing.
- A Luau error in `run` is logged with the system name and the system is skipped that tick; the game keeps running. Read
  the log after every run.
- Tune through data (a balance module, component defaults), never magic numbers.

## Setup forms, calls, inputs

Declarative setup at load time (same code both sides; a server gets ids without loading anything):
`engine.assets.texture/atlas/sound/font/tilemap(...)` return asset ids; `engine.input.action(name, type, {bind})`
declares inputs clients send. Other plugin features are engine calls: `engine.call("aylo.x.y", {args})` at load time,
`ctx.call` inside a declared system. `engine.hasCall("...")` tests availability. The call list is in
`manual/pages/engine-calls.md`.

## Method

1. Read the design doc (`docs/design/`) of the ticket; restate rules and tuning values in your own plan. If numbers are
   missing, ask via `comment_ticket` / `create_ticket` for `gamedesign` rather than inventing.
2. Write down first: components (which side, replication), inputs, systems (phase, side, query, calls), settings.
3. Shared rules and declarations in `shared/`; authority and gameplay in `server/`; presentation in `client/`.
4. Run a server and a client, play it. Read the output for system errors. Fix. Repeat.
5. Missing API? `raise-engine-gap`. Manual wrong or thin? fix `manual/` in the same change (`write-manual`).
6. Commit atomically (`commit-conventions`), report honestly (`verify`).

## Pitfalls checklist

- A file nobody `require`s never runs (no warning).
- A client system writing a replicated component: the server wins; do it with an input instead (`server-authority`).
- Client-only plugin names under the server entry fail to load; keep presentation under `client/`.
- Typo in a `project.luau` key stops the game at start - that is the feature.
- A `vec2` assigned with a missing key.
- `require` inside `run`.
- Using a global to share state (each file has its own globals).
