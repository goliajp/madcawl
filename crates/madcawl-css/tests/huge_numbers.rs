//! A number too large for an f64 is the largest finite one, so colours
//! built from it stay finite.

use madcawl_css::Span;
use madcawl_css::color::{ColorValue, parse_color};

fn channels(src: &str) -> Vec<f64> {
    match parse_color(src, Span::new(0, src.len())) {
        Some(ColorValue::Color(c)) => c.channels().iter().flatten().copied().collect(),
        other => panic!("{src}: {other:?}"),
    }
}

#[test]
fn an_overflowing_number_is_clamped_to_a_finite_value() {
    for src in ["lch(50 1 -1e999)", "lab(1e999 0 0)", "oklch(0.5 0.1 1e999)"] {
        let ch = channels(src);
        assert!(ch.iter().all(|v| v.is_finite()), "{src}: {ch:?}");
    }
}
