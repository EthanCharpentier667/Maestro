---
name: unity-gameplay
description: Conventions for Unity (C#, URP) gameplay code. Use when editing .cs files, prefabs or ScriptableObjects in a Unity project.
---

# Unity gameplay conventions

- One MonoBehaviour per responsibility; tunables in ScriptableObjects or `[SerializeField] private` fields.
- No `Find*`/`GetComponent` in `Update`; cache references in `Awake`.
- Use the new Input System if the project has it; physics in `FixedUpdate`.
- Never hand-edit `.meta` GUIDs; when creating assets from text, keep YAML valid.
- Namespaces match folders (`Game.Player`, `Game.AI`…). Avoid allocations per frame (LINQ, string concat).
