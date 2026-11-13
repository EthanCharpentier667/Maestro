---
name: cpp-engine
description: Architecture rules, dependency graph, code style and error handling for the AYLO C++23 engine (xmake, clang, ECS, Vulkan, Luau bridge, network). Use when editing anything under src/, tests/ or xmake files of the engine repository.
---

# AYLO engine - C++ work

AYLO is a C++23 game engine built with `xmake` + clang: ECS, plugin libraries, Vulkan renderer, Luau scripting,
physics, audio, server-authoritative networking. **It contains no game**: a game is a folder of Luau run by the generic
`aylo-runtime`. The repo's own instructions are the source of truth and you read them every time:

1. root `AGENTS.md` (architecture rules, dependency rules, definition of done),
2. `src/AGENTS.md`, then the `AGENTS.md` of the module (`src/aylo/<module>/AGENTS.md`) you edit,
3. `docs/CodeStyle.md`, and the design doc of the module (`docs/Modules.md`, `docs/Plugins.md`, `docs/<module>/design.md`).

If this skill and the repo disagree, the repo wins and this skill is stale: say so in your summary.

## The ten architecture rules (root AGENTS.md)

1. **Engine is not game.** No gameplay, no game name in the engine. A game is only `project.luau` + `scripts/` + `assets/`.
2. **The Luau API is the product.** A feature a game needs is not done until a script can use it, the manual
   (`manual/`) documents it and a script test covers it. A script-visible change without a `manual/` change does not merge.
3. **Data over code.** Plugins, backends, window, network, physics are chosen in `project.luau`, never in a
   game-specific `main`.
4. **Dependencies point inward.** Contracts know no ECS, backends know only their contract, plugins know no backend.
5. **A plugin stands alone.** It owns its data, names the plugins it requires, never adds another; no cycle.
6. **The server is authoritative.** Every datum says which side it lives on; a client never writes replicated data;
   bytes from the network never crash, throw or assert (see `server-authority`).
7. **A third-party library is private** to the one module wrapping it; no public header includes it.
8. **No global state.** State lives in the world.
9. **Errors.** A failure a correct program can hit is a returned result; misuse throws; a script error never takes the
   game down.
10. **Portable.** Linux, macOS, Windows; Clang, Apple Clang, MSVC must compile it (GCC should).

## Module map and who may depend on whom

```
core <- ecs, contract-*
contract-* <- platform-*, render-*, audio-*     (loaded by the engine)
ecs, contract-* <- plugin-*                     (loaded by the engine)
luau <- scripting ; network <- replication ; ecs <- replication, scripting
scripting, replication <- engine <- executables, examples
```

- `core` depends on no project target; `ecs` on `core` only.
- `luau`, `network`, `replication`, `scripting`, `engine` are **static**: no plugin/backend/contract may link them (the
  build refuses it, rule `aylo.static_modules`). `ecs` and the contracts stay shared.
- `network` knows Asio only; `replication` is the only module knowing both ECS and network.
- A plugin depends on `core`, `ecs`, glm, contracts, the plugins it requires, plus private packages. **Never** on a
  `platform-*`, `render-*`, `audio-*` or `luau` target.
- Backends link their contract only; what a renderer needs from a window goes through `interop-*`.
- `engine` links no plugin and no backend: it loads them from libraries as `project.luau` names them.
- Before adding an `add_deps`/`#include` that crosses a boundary, check it against this graph. If it does not fit, the
  design is wrong, not the graph - open a ticket asking for a design decision.

## Code style (docs/CodeStyle.md; the tools enforce the mechanical part)

- Google C++ style with changes: 4 spaces, 120 columns, newline at EOF. **Run `clang-format -i`, do not hand-format.**
- Files in PascalCase after their class: `Window.hpp`, `Window.cpp`, `.tpp` for template bodies. `#pragma once`, never
  `#ifndef` guards, right after the Epitech header:
  ```cpp
  /*
  ** EPITECH PROJECT, 2026
  ** aylo
  ** File description:
  ** Window
  */
  ```
- Namespaces mirror the path under `src/aylo/` (`aylo::plugins::physics`). No `using namespace`.
- Includes: a sibling by bare name, anything else as `aylo/<module>/...`. Never `../`.
- Every shared library marks its exports with its `AYLO_<LIB>_API` macro; a static module keeps the macro and defines
  `AYLO_<LIB>_STATIC` publicly; a type with no code of its own uses the header-only macro of `core`.
- RAII, no owning raw pointers, rule of five when you own a resource, `const`/`noexcept` where true, no
  `std::shared_ptr` unless ownership is really shared.
- clang-tidy groups enforced: `bugprone`, `cppcoreguidelines`, `clang-analyzer`, `modernize`, `performance`,
  `readability`, `misc`. "reports nothing new" is the bar; do not add `NOLINT` without a one-line reason.

## Error handling

- Misuse and construction failures **throw** exceptions derived from `std::runtime_error`, kept in an `exceptions/`
  folder next to the code that throws them.
- `luau` and `scripting`: a failure a correct program can hit returns a result (`docs/luau/error-handling.md`);
  exceptions only for misuse.
- Anything fed by a network peer, a file or a script is untrusted: validate length, range and enum values; return an
  error, never crash/assert.
- A Luau error inside a system is logged with the system name and the system is skipped for that tick (D8).

## ECS and data-oriented rules

- Components are plain structs (defaults on fields, `///<` docs), no logic. Systems iterate queries.
- Declare reflected data with `AYLO_ECS_COMPONENT` / `AYLO_ECS_RESOURCE` / `AYLO_ECS_EVENT` and a stable `aylo.Name`;
  unlisted fields are invisible to scripts, serialization and replication. The layout is part of the network manifest.
- No virtual calls and no allocations in hot loops; measure (`docs/benchmarks`) before optimising, and say what you
  measured in the commit body.
- Structural changes go through `Commands` (deferred).

## Working method

1. Re-sync the repo (`engine-repo-sync`), read the AGENTS.md chain, find the closest existing implementation and copy
   its shape rather than inventing one.
2. Keep the change inside one module when possible. If you must cross a boundary, justify it in the commit body.
3. Build and test locally (`verify`), commit atomically (`commit-conventions`).
4. Script-visible? Chain the right skill: `add-plugin` / `add-component` / `add-engine-call`, then `write-manual`.
5. Done means: build, tests, format/lint clean, docs updated, honest summary.
