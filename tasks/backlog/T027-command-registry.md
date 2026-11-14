# T027 Command registry skeleton

**Crate:** `cad-commands`
**Goal:** The single path for document edits (rule 3, D5), with the first commands.
**Public API:** `trait Command { const ID: &str; type Args; fn validate(&Document, &Args) -> Result<(), CommandError>; fn apply(&mut Transaction, Args) -> Result<Change, CommandError>; }`, a `Registry` listing commands by ID with label and description, and an `Args` description (field names, types, units) the CLI, MCP, and UI can read.
**Acceptance:**
1. Commands: `parameter.set`, `parameter.remove`, `feature.box`, `feature.extrude`, `feature.fillet`, `feature.delete`, `edit.undo`, `edit.redo`.
2. `validate` failing leaves the document unchanged and returns an error naming the argument.
3. Every command applies inside a transaction; a failing `apply` rolls back.
4. The registry lists every command with its argument description; IDs are unique and the list is in a stable order.
5. Applying a command and rebuilding gives the same volume as building the document directly in `cad-doc`.
**Out of scope:** previews, JSON schemas and serialization (need a dependency task that justifies serde), hotkeys, icons, selection rules.
**Dependencies:** none.
**Depends on:** T025, T026.
