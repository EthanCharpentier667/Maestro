---
name: add-component
description: Add a component, resource or event to an AYLO engine plugin so C++, scripts, serialization and replication all see it. Use when giving a plugin new data.
---

# Add a component, resource or event

Read `src/aylo/plugins/AGENTS.md`, `src/aylo/ecs/AGENTS.md`, `docs/ecs/features/02-component-registry.md`, `10-resources.md`,
`11-events.md`, `15-reflection-and-serialization.md`, `docs/CodeStyle.md`. Then `engine-repo-sync`.

## Checklist

1. **Place it** in the plugin that owns it (`src/aylo/plugins/<name>/`), in that plugin's namespace. Data several
   unrelated modules need goes to `core`; data a contract needs is plain data in the contract, with its ECS
   declaration in the plugin that uses it. Never register data from a plugin that does not own it.
2. **Plain struct**: fields with defaults, documented with `///<`. Reflected fields must be of a type the ECS can
   describe: scalars, `std::array` of them, glm vectors (include the glm reflection header of `plugins/`), entities,
   asset ids, and ECS strings. Another type needs a reflection like the existing ones (`plugins/GraphicsReflection.hpp`,
   the audio reflection) - do not expose it unreflected.
3. **Declare it** right after the struct with the macro of its kind and a stable name: `AYLO_ECS_COMPONENT(Type,
   "aylo.Name", fields...)`, `AYLO_ECS_RESOURCE(...)`, `AYLO_ECS_EVENT(...)`. Unlisted fields are stored but invisible to
   scripts, serialization and replication. A struct with no field is a tag.
4. **Say where it lives**: server only, client only, or both; replicated never / on every change / at spawn. The
   server is authoritative; a client never writes replicated data. Choose the least that is true (`server-authority`).
5. **Register** in the plugin's build step, before systems and scripts use it. If it needs another plugin's data, the
   plugin already requires that plugin.
6. **Reachable from Luau**: registered components are visible by name with their reflected fields. Verify by writing the
   script. If it needs something data alone cannot do (load an asset, call C++), it also needs an engine call
   (`add-engine-call`).
7. **Tests**: C++ in `tests/plugins/` (defaults, registration, side); script test in `tests/engine/` reading/writing it
   from Luau; if it replicates, a test in `tests/replication/`.
8. **Docs**: feature page of the plugin in `docs/`; the component table in `manual/` via `write-manual` (fields, types,
   defaults, side, example); `docs/Modules.md` if it is a notable class.
9. `verify`, `commit-conventions`.

## Pitfalls

- **A name changed later breaks saved scenes (capsules) and the network manifest**: choose it once.
- The layout of a reflected struct is part of the network manifest; adding or reordering fields changes it on both sides
  at once. Announce a layout change in the commit body and in the ticket.
- Renaming/removing a field breaks games: this is a non-additive change -> `notify-game-on-engine-update`.
