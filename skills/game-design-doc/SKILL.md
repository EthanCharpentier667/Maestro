---
name: game-design-doc
description: Template and rules for writing game design documents (mechanics, systems, levels). Use when creating or editing anything in docs/design/.
---

# Game design doc format

File: `docs/design/<system-or-feature>.md`

```markdown
# <Feature name>
## Intent            — the player fantasy / feeling in 1-3 sentences
## Rules             — numbered, unambiguous rules
## Controls & feedback — inputs, VFX/SFX/UI feedback
## Tuning            — table: parameter | value | unit | notes
## Edge cases        — what happens when…
## Acceptance criteria — testable bullet list for programmers
## Open questions
```

- Use concrete numbers in Tuning (start values), never "fast"/"a lot".
- Cross-link related docs; keep one doc per system.
