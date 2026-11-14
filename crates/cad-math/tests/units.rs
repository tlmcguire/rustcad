//! Acceptance tests for T009: units and quantities.

use std::f64::consts::PI;

use cad_math::{Dimension, Quantity, Tolerance, Unit, UnitError};

type TestResult = Result<(), UnitError>;

fn tol() -> Tolerance {
    Tolerance::default()
}

#[test]
fn an_inch_is_25_4_mm() -> TestResult {
    let inch = Quantity::parse("1 in")?;
    let mm = Quantity::parse("25.4 mm")?;
    assert!(inch.approx_eq(mm, tol()), "{inch:?} vs {mm:?}");
    assert!(tol().eq_len(Quantity::parse("1 ft")?.to(Unit::In)?, 12.0));
    assert!(tol().eq_len(Quantity::parse("2 m")?.to(Unit::Cm)?, 200.0));
    Ok(())
}

#[test]
fn degrees_are_stored_as_radians() -> TestResult {
    let half_turn = Quantity::parse("180 deg")?;
    assert_eq!(half_turn.dim, Dimension::ANGLE);
    assert!(tol().eq_angle(half_turn.value, PI));
    assert!(tol().eq_angle(half_turn.to(Unit::Deg)?, 180.0));
    assert!(half_turn.approx_eq(Quantity::rad(PI), tol()));
    Ok(())
}

#[test]
fn lengths_are_stored_in_millimetres() -> TestResult {
    for (text, mm) in [
        ("40 mm", 40.0),
        ("4 cm", 40.0),
        ("0.04 m", 40.0),
        ("2 in", 50.8),
    ] {
        let q = Quantity::parse(text)?;
        assert_eq!(q.dim, Dimension::LENGTH, "{text}");
        assert!(tol().eq_len(q.value, mm), "{text}: {}", q.value);
    }
    Ok(())
}

#[test]
fn adding_a_length_to_an_angle_names_both_dimensions() -> TestResult {
    let length = Quantity::parse("3 mm")?;
    let angle = Quantity::parse("30 deg")?;
    let err = length.checked_add(angle).err();
    assert_eq!(
        err,
        Some(UnitError::Mismatch {
            left: Dimension::LENGTH,
            right: Dimension::ANGLE,
        })
    );
    let text = err.map(|e| e.to_string()).unwrap_or_default();
    assert!(text.contains("length") && text.contains("angle"), "{text}");
    assert!(angle.checked_sub(length).is_err());
    Ok(())
}

#[test]
fn like_quantities_add_and_subtract() -> TestResult {
    let sum = Quantity::parse("1 in")?.checked_add(Quantity::parse("4.6 mm")?)?;
    assert!(sum.approx_eq(Quantity::mm(30.0), tol()));
    let diff = Quantity::parse("90 deg")?.checked_sub(Quantity::parse("45 deg")?)?;
    assert!(tol().eq_angle(diff.value, PI / 4.0));
    Ok(())
}

#[test]
fn lengths_multiply_to_area_and_divide_to_a_number() -> TestResult {
    let (a, b) = (Quantity::mm(4.0), Quantity::mm(5.0));
    let area = a.checked_mul(b)?;
    assert_eq!(area.dim, Dimension::AREA);
    assert!(tol().eq_len(area.value, 20.0));
    assert_eq!(area.dim.to_string(), "area");
    assert_eq!(area.checked_mul(a)?.dim, Dimension::VOLUME);

    let ratio = a.checked_div(b)?;
    assert_eq!(ratio.dim, Dimension::DIMENSIONLESS);
    assert!(tol().eq_len(ratio.value, 0.8));

    // Mixed products are dimensions too, with a readable name.
    let mixed = a.checked_mul(Quantity::rad(1.0))?;
    assert_eq!(mixed.dim, Dimension::new(1, 1));
    assert_eq!(mixed.dim.to_string(), "length * angle");
    assert_eq!(Dimension::new(-2, 0).to_string(), "length^-2");
    Ok(())
}

#[test]
fn scaling_by_a_number_keeps_the_dimension() {
    let q = Quantity::mm(10.0);
    assert_eq!((q * 2.0).dim, Dimension::LENGTH);
    assert!((q / 4.0).approx_eq(Quantity::mm(2.5), tol()));
    assert!((-q).approx_eq(Quantity::mm(-10.0), tol()));
}

#[test]
fn converting_to_a_unit_of_another_dimension_is_an_error() -> TestResult {
    let length = Quantity::parse("10 mm")?;
    assert_eq!(
        length.to(Unit::Deg),
        Err(UnitError::Mismatch {
            left: Dimension::ANGLE,
            right: Dimension::LENGTH,
        })
    );
    assert!(Quantity::parse("1 rad")?.to(Unit::Mm).is_err());
    assert!(Quantity::number(3.0).to(Unit::Mm).is_err());
    assert!(
        length.checked_mul(length)?.to(Unit::Mm).is_err(),
        "area is not a length"
    );
    Ok(())
}

#[test]
fn parsing_accepts_optional_whitespace() -> TestResult {
    for text in [
        "40mm",
        "40 mm",
        "  40   mm  ",
        "\t40\tmm\n",
        "+40 mm",
        "4e1 mm",
        "40.0mm",
    ] {
        let q = Quantity::parse(text)?;
        assert!(q.approx_eq(Quantity::mm(40.0), tol()), "{text:?}: {q:?}");
    }
    assert!(Quantity::parse("-1.5e2deg")?.approx_eq(Quantity::rad(-150.0 * PI / 180.0), tol()));
    assert!(Quantity::parse(".5 in")?.approx_eq(Quantity::mm(12.7), tol()));
    Ok(())
}

#[test]
fn a_number_without_a_unit_is_dimensionless() -> TestResult {
    let q = Quantity::parse(" 3 ")?;
    assert_eq!(q.dim, Dimension::DIMENSIONLESS);
    assert!(tol().eq_len(q.value, 3.0));
    Ok(())
}

#[test]
fn unknown_units_are_reported_with_their_text() {
    assert_eq!(
        Quantity::parse("40 furlong"),
        Err(UnitError::UnknownUnit("furlong".into()))
    );
    // Units are case-sensitive, and a unit is the whole rest of the text.
    assert_eq!(
        Quantity::parse("40 MM"),
        Err(UnitError::UnknownUnit("MM".into()))
    );
    assert_eq!(
        Quantity::parse("40 mm mm"),
        Err(UnitError::UnknownUnit("mm mm".into()))
    );
    // `2em` is not 2e(xponent) followed by `m`.
    assert_eq!(
        Quantity::parse("2em"),
        Err(UnitError::UnknownUnit("em".into()))
    );
    assert_eq!(Unit::parse("yd"), Err(UnitError::UnknownUnit("yd".into())));
}

#[test]
fn text_without_a_finite_number_is_rejected() {
    for text in [
        "", "   ", "mm", "inf mm", "NaN", "- 3 mm", ".", "e5 mm", "1e999 mm",
    ] {
        assert!(
            matches!(Quantity::parse(text), Err(UnitError::InvalidNumber(_))),
            "{text:?}: {:?}",
            Quantity::parse(text)
        );
    }
}

#[test]
fn every_unit_parses_from_its_symbol() -> TestResult {
    for unit in Unit::ALL {
        assert_eq!(Unit::parse(unit.symbol())?, unit);
        let q = Quantity::parse(&format!("2 {unit}"))?;
        assert_eq!(q.dim, unit.dimension());
        assert!(tol().eq_len(q.to(unit)?, 2.0), "{unit}");
    }
    Ok(())
}
