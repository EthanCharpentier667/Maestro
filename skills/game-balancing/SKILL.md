---
name: game-balancing
description: Method for balancing numbers in AYLO games - tuning tables, formulas, derived values, ranges, sanity simulations and balance data modules. Use when setting or changing damage, speeds, timers, costs, spawn rates, progression curves or any tunable.
---

# Game balancing

Balance is the discipline of making numbers explainable. Every tunable has a reason, a unit, a range, and a place.

## Where the numbers live

- The design doc's **Tuning table** is the source of truth (`game-design-doc`).
- In the game, tunables live in **one shared module** returning a table
  (`scripts/shared/balance.luau`: `return { ship = { speed = 4, hp = 100 }, laser = { damage = 25, cooldown = 0.4 } }`),
  required by whoever needs it (`luau-modules`). Keys mirror the table: `balance.laser.cooldown`.
  Component `defaults` may read from it. No magic numbers in systems.
- You may edit that module and other data files; you do not touch system logic. If a number needs a new rule, write
  the rule in the doc and let `gamedev` implement it.

## Method

1. **State the target in player terms**: time-to-kill, round length, expected comeback chance, resource income per
   minute. A number without a target cannot be judged.
2. **Pick anchors** (one or two numbers that define the scale: base HP, base speed) and derive the others by formula, so
   changing an anchor rescales coherently. Write the formula in the doc: `ttk = hp / (damage * fireRate)`.
3. **Check units and tick math.** The simulation steps at a fixed delta (`ctx.fixedDelta`); a cooldown of 0.4 s at 60
   Hz is 24 ticks. Prefer seconds/units-per-second in data, convert in code.
4. **Sanity-simulate in a table**: compute the extremes (best/worst case, stacked modifiers, 1 vs N players) by hand or
   with a small script and paste the results. Look for dominant strategies, zero-cost options, infinite loops, and
   values that overflow a field type (`u8` caps at 255, `f32` precision at large coordinates).
5. **Bound every value** with min/max and say what happens at the bounds.
6. **Change one lever at a time** and log it in the doc's changelog (`date · parameter old -> new · why`).
7. **Network sense**: values that replicate every change cost bandwidth; prefer deriving on the client from a replicated
   base + start time rather than replicating a ticking value.

## Output

A Tuning table with `parameter | value | unit | range | derived-from / why`, a short "expected outcomes" list (the
targets and the numbers that produce them), and the balance module diff. Flag anything that needs playtesting as an
**Open question** with a proposed default so engineering is not blocked.

## Pitfalls

- Duplicate numbers in two docs or two modules - they drift.
- Percent vs fraction (`0.25` vs `25`).
- Balance by feel with no stated target.
- Tuning to hide a bug (a speed that "works" because collisions are broken): file a ticket instead.
