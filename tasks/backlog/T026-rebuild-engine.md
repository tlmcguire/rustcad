# T026 Rebuild engine, sequential

**Crate:** `cad-eval`
**Goal:** Evaluate a document into bodies through the `Kernel` trait, with caching (PLAN 8.5, items 1, 2, 3, 6).
**Public API:** `Evaluator::new(&dyn Kernel)`, `Evaluator::rebuild(&Document) -> Evaluation`, `Evaluation` with per-feature `FeatureStatus { Ok, Warning(msg), Failed(error), Suppressed }` and output shape.
**Acceptance:**
1. Box, sketch, extrude, fillet, and boolean features evaluate in dependency order; parameter expressions are resolved first.
2. A failed feature marks its dependants failed and does not stop independent features.
3. Results are cached by a hash of the feature's resolved arguments and input shape hashes. Changing a parameter and changing it back performs no kernel calls on the second rebuild (counted with a wrapping test kernel).
4. Unchanged upstream features are not re-evaluated after a downstream edit.
5. Two rebuilds of the same document give identical volumes and statuses (rule 9). Hashing uses a fixed algorithm, not one seeded per process.
6. Kernel shapes no longer reachable from the cache are released.
**Out of scope:** parallel evaluation and cancellation (need a thread pool dependency task), coarse previews.
**Dependencies:** none.
**Depends on:** T023, T024, T016, T018, T022.
