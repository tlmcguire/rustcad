# T010 Expression parser and evaluator

**Crate:** `cad-math`
**Goal:** Any numeric field can hold an expression such as `width / 2 + 3 mm` (PLAN 8.3).
**Public API:** `Expr::parse(&str) -> Result<Expr, ExprError>`, `Expr::references() -> Vec<String>`, `Expr::eval(&dyn Fn(&str) -> Option<Quantity>) -> Result<Quantity, ExprError>`.
**Acceptance:**
1. Operators `+ - * /`, unary minus, parentheses, and the usual precedence. `2 + 3 * 4` is 14.
2. Functions `sin cos tan sqrt abs min max`. Trigonometric functions take angles; `sin(30 deg)` is 0.5 and `sin(3 mm)` is a unit error.
3. Literals may carry units (`3 mm`, `45 deg`). A bare number is dimensionless; in a length field it is read as mm by the caller, not here.
4. An unknown name returns `ExprError::UnknownName`. A parse error carries the byte offset.
5. `references()` lists names in order of first appearance, without duplicates.
6. Division by zero returns an error, not infinity.
**Out of scope:** cycle detection (T012), conditionals, user functions, string values.
**Dependencies:** none. Hand-written parser.
**Depends on:** T009.
