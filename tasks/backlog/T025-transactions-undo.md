# T025 Transactions and undo

**Crate:** `cad-doc`
**Goal:** Every edit runs in a transaction; undo restores the previous snapshot (PLAN 5.5, 8.7).
**Public API:** `History` holding the current `Document` and an undo and redo stack; `History::begin() -> Transaction`, `Transaction::commit(label)`, `rollback`, nested `begin` inside a transaction; `undo`, `redo`; a `Change` value listing affected feature IDs.
**Acceptance:**
1. Commit pushes one undo entry. Rollback leaves the document and stacks unchanged.
2. A nested transaction's commit only takes effect when the outermost commits, as one undo entry.
3. Undo then redo returns a document equal to the one before undo.
4. A new commit after undo clears the redo stack.
5. Each commit returns the set of changed feature IDs, for subscribers.
6. Dropping a transaction without commit rolls back.
**Out of scope:** the journal and autosave, threads and the single-writer rule (enforced later in `cad-commands`).
**Dependencies:** none.
**Depends on:** T023.
