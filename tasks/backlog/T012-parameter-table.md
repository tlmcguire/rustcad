# T012 Parameter table

**Crate:** `cad-doc`
**Goal:** Named parameters with expressions, evaluated in dependency order (PLAN 8.3).
**Public API:** `Parameters` with `set(name, expr_text) -> Result<(), ParamError>`, `remove(name)`, `value(name) -> Result<Quantity, ParamError>`, `evaluate_all() -> Result<BTreeMap<String, Quantity>, ParamError>`.
**Acceptance:**
1. `width = 40 mm`, `half = width / 2` gives `half` 20 mm.
2. A cycle (`a = b`, `b = a + 1`) is rejected by `set` with `ParamError::Cycle` listing the names in the cycle, and the table is unchanged.
3. Removing a parameter another depends on returns `ParamError::InUse` naming the dependants.
4. Names follow one rule: ASCII letter, then letters, digits, or underscore. Function names from T010 are reserved.
5. `evaluate_all` is deterministic: same table, same map, regardless of insertion order.
**Out of scope:** units on feature fields, persistence, undo.
**Dependencies:** none.
**Depends on:** T010.
