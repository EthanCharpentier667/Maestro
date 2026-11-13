---
name: engine-feature
description: Engine feature end to end - engine implements the API with tests, then docs writes the manual page, on the same branch.
lead: engine
members: [docs]
mode: pipeline
color: '#8b5cf6'
labels: [engine-feature]
---

A script-visible engine feature is not done until the manual explains it and a script test covers it.

- Step 1 (`engine`): implement the feature (`add-plugin` / `add-component` / `add-engine-call`), with C++ tests and a
  script test. Leave the manual to the next step but list exactly what changed for scripts: call names, arguments with
  defaults, result, component fields, side.
- Step 2 (`docs`): write or update the manual pages from the code and tests (`write-manual`), correct stale plan docs,
  and verify the examples against the script tests.
