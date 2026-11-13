---
name: add-plugin
description: Add a new engine plugin (an ECS plugin library under src/aylo/plugins) to AYLO, from the xmake target to Luau reachability, tests and manual. Use when a feature needs its own components, resources, events and systems.
---

# Add an engine plugin

A plugin is one engine feature as an ECS plugin: one shared library `plugin-<name>` in `src/aylo/plugins/<name>/`.
Read `src/aylo/plugins/AGENTS.md`, `docs/Plugins.md` (graph and rules), `docs/CodeStyle.md`; copy the structure of a
small plugin of the same shape (shape, sprite).

## Checklist

1. **Boundaries.** What does it own (components, resources, events, systems)? Which plugins does it require? Allowed
   dependencies: `core`, `ecs`, glm, contracts, required plugins, private packages. Never a `platform-*`, `render-*`,
   `audio-*` or `luau` target; no cycle in the plugin graph. Which side runs what (drawing = client; gameplay data
   states server/client/both).
2. **Folder** `src/aylo/plugins/<name>/`, namespace `aylo::plugins::<name>`, a feature subfolder only when files go in
   it, an export header for `AYLO_PLUGIN_<NAME>_API`, exceptions in `exceptions/`.
3. **Target** in `src/aylo/plugins/xmake.lua` with `plugin("<name>", {<deps>})`: sources/headers by glob, private
   packages with `add_packages`. The helper applies `aylo.plugin_library`. A new third-party package: `add_requires`
   there (or in the root file if several modules use it), private, no public header includes it.
4. **Plugin class**: declares requirements in the dependencies it reports, registers data in its build step, never adds
   another plugin (the program adds them, requirements first). Rule of five and the code style.
5. **Data** with the ECS macros (`add-component`), names in the `aylo.` namespace.
6. **Settings**: if it reads a `project.luau` section, read through the shared settings, refuse unknown keys, validate
   values before the app is built.
7. **Loadable and reachable**: a `Library.cpp` with one `AYLO_PLUGIN_LIBRARY` registering the plugin under its
   `getName()` (`aylo.<Name>`) (`docs/Plugins.md`, *Entry points*). A game lists that name under `plugins`; no
   executable or engine file changes. Register its engine calls from the build step (`add-engine-call`).
8. **Tests** in `tests/plugins/`, one file per concern; add the plugin to `plugins-tests` deps in
   `tests/plugins/xmake.lua`; success + failure paths + the requirement check.
9. **Script test** in `tests/engine/` for what scripts can do with it.
10. **Docs**: tables in `docs/Plugins.md`; a design page (or section) with "From scripts"; `docs/Modules.md`; a manual
    page via `write-manual` (nav in `manual/mkdocs.yml`), no C++ in it.
11. `verify`; commit order: build file + plugin skeleton, data, calls, tests, docs (`commit-conventions`).
12. `notify-game-on-engine-update` if games now have something new worth adopting.

## Done means

A script can use the feature, the manual explains it, a script test covers it, `docs/Plugins.md` lists it, and the
dependency rules still hold (check against the graph in `cpp-engine`).
