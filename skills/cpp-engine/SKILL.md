---
name: cpp-engine
description: Conventions for C++ engine code (C++20, CMake, ECS, networking). Use when editing C/C++ sources or CMake files.
---

# C++ engine conventions

- C++20, RAII everywhere, no owning raw pointers (`std::unique_ptr`/`std::shared_ptr` only where shared).
- Headers: `#pragma once`; includes ordered std → third-party → project.
- ECS: components are plain structs (no logic); systems iterate views; no virtual calls in hot loops.
- Networking: fixed-size, versioned packets; validate every length/field received; never trust the client.
- Build: `cmake -B build -DCMAKE_BUILD_TYPE=Release && cmake --build build -j` must succeed with no new warnings.
- Tests: add/adjust unit tests (Criterion/GoogleTest/Catch2 — whatever the repo uses).
