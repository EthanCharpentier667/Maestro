---
name: feature-pipeline
description: Design first, then implementation - gamedesign writes the spec in docs/design, gamedev implements it in Luau on the same branch.
lead: gamedesign
members: [gamedev]
mode: pipeline
color: '#06b6d4'
labels: [pipeline]
---

Each step builds on the previous one on the same branch. Read the previous steps' summaries and the files they changed
before starting.

- Step 1 (`gamedesign`): produce `docs/design/<system>.md` with numbered rules, the data model (components, side,
  replication, inputs), a Tuning table and testable acceptance criteria. List any **Engine need** as an open question.
- Step 2 (`gamedev`): implement exactly the agreed doc in Luau. Do not change rules or numbers; if the doc is wrong or
  incomplete, say so in your summary and comment on the ticket rather than silently deviating. If an Engine need
  blocks you, file it (`raise-engine-gap`) and deliver what is possible.
