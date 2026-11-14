# T009 Units and quantities

**Crate:** `cad-math`
**Goal:** Typed quantities so that expressions and features can check units (PLAN 8.3.3). Internal units follow D10.
**Public API:** `Dimension` (`Length`, `Angle`, `Dimensionless`, and products such as area), `Quantity { value: f64, dim: Dimension }`, `Unit` (`mm`, `cm`, `m`, `in`, `ft`, `rad`, `deg`), `Quantity::parse("40 mm")`, `Quantity::to(Unit) -> Result<f64, UnitError>`.
**Acceptance:**
1. `25.4 mm` equals `1 in` under `Tolerance`. `180 deg` stores `PI` radians.
2. Adding a length to an angle returns `UnitError::Mismatch` naming both dimensions.
3. Multiplying two lengths gives an area. Dividing a length by a length gives a dimensionless value.
4. Converting a length to `deg` is an error.
5. Parsing accepts optional whitespace and rejects unknown units with a typed error that carries the unit text.
**Out of scope:** mass, density, temperature, user-defined units, formatting for display.
**Dependencies:** none.
**Depends on:** none.
