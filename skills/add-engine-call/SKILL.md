---
name: add-engine-call
description: Add an engine call (aylo.<plugin>.<verb>) or setup form so a Luau script can use a plugin feature, with data structs, registration, C++ and script tests, and docs. Use when asked to expose something to Luau, add a call, or add a setup form.
---

# Add an engine call

An engine call is a named function a plugin registers; scripts invoke it with a table of arguments
(`engine.call("aylo.audio.loadSound", {...})` while loading, `ctx.call(...)` in a system that declared it). It is how a
game reaches what plain data cannot do: load an asset, bind an input, play a sound, query physics.

**The rule: a feature is not done until a script can use it, the manual explains it and a script test covers it.**

Read first: `docs/ecs/features/19-luau-bridge.md` (*Engine calls*, *Setup forms*), `docs/ecs/features/12-systems.md`,
`docs/scripting/plan.md`, and one existing example (audio, input or sprite plugin's `*Calls.hpp/.cpp`). Run
`engine-repo-sync` and `plan-vs-implemented` first - the call may exist, or its design may be undecided.

## Checklist

1. **Name** `aylo.<plugin>.<verb>`, camelCase verb. One call does one thing. A published name is a public API: scripts
   depend on it, choose once.
2. **Define the data**: an argument struct and, if it returns something, a result struct, each declared with the
   call-data macro and a stable `aylo.<plugin>.<Name>`, in `<Name>Calls.hpp`. **Every argument field has a sensible
   default** (missing fields keep it). Types: asset-id fields for assets (scripts pass a path), `ecs::String` (255
   bytes) for short text, `std::string` for text that may be long, `std::vector` for lists, glm vectors for
   positions. Optional values use a documented sentinel (NaN for "not given"), stated in the field comment.
3. **Write the function.** Its parameters are system parameters (resources, queries, event writers), so the call
   declares its access and the scheduler orders it. Register from `add<Name>Calls()` in `<Name>Calls.cpp`, called from
   the plugin's build step. **Failures raise the call exception with a message naming the call; never crash, never
   assert on script input.** A call on a side without the plugin is absent there, and scripts/forms skip it.
4. **Setup forms** if it is part of declaring a game's setup: extend `engine.assets` / `engine.input` in the plugin's
   `<Name>Forms.luau` (listed in `src/aylo/plugins/xmake.lua`, registered from its library and build step, written with
   the `engine.forms` helpers of `scripting/setup/FormsPrelude.luau`). Forms stay thin over the call and test
   `engine.hasCall()`.
5. **Tests**
   - C++ in `tests/plugins/`: success, bad argument, the side where it is absent.
   - Script test in `tests/engine/` (model: `ScriptEngineCallsTests`, `ScriptSetupFormsTests`, `ScriptPhysicsTests`): a
     Luau snippet on a real app asserting results **and error messages**.
6. **Docs**: invoke `write-manual`; add the call to the plugin page in `manual/` (name, arguments with types/defaults,
   result, side, example script, errors), to the calls table of `19-luau-bridge.md`, and to the "From scripts" section
   of the plugin's design doc.
7. `verify`, then commits in this order (`commit-conventions`): data + function, registration, C++ tests, script test,
   docs.
8. If the call lifts a limitation the game worked around: `notify-game-on-engine-update`.

## Pitfalls

- A system must list a call in `calls` before `ctx.call`; an unknown call in a system that does not run in the app's
  mode is accepted, in any other it is refused at load. Document which side registers it.
- Do not special-case a plugin inside `scripting`: the generic call mechanism is the only path.
- Changing a call's argument or result later is a breaking change for games: prefer adding a new optional field.
- Replicated or server-authoritative effects belong on the server side only (`server-authority`).
