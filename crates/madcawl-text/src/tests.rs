use crate::*;
use std::sync::Arc;

fn u16s(s: &str) -> Vec<u16> {
    s.encode_utf16().collect()
}

fn style(shaper: &mut Shaper, family: &str) -> Style {
    Style {
        face: shaper.fonts.select(&[family], 400.0, false),
        size: 16.0,
        features: 0,
    }
}

#[test]
fn selects_installed_faces() {
    let mut fonts = Fonts::new();
    let regular = fonts.select(&["No Such Family", "Helvetica"], 400.0, false);
    assert_eq!(fonts.family_name(regular), "Helvetica");
    let bold = fonts.select(&["Helvetica"], 700.0, false);
    assert_ne!(bold, regular);
    assert!(
        fonts.postscript_name(bold).contains("Bold"),
        "{}",
        fonts.postscript_name(bold)
    );
    assert_eq!(fonts.select(&["Helvetica"], 400.0, false), regular);
    let mono = fonts.select(&["monospace"], 400.0, false);
    assert_eq!(fonts.family_name(mono), "Menlo");
    let m = fonts.metrics(regular);
    assert!(
        m.ascent > 0.5 && m.descent > 0.0 && m.units_per_em > 0,
        "{m:?}"
    );
    let missing = fonts.select(&["No Such Family"], 400.0, false);
    assert_eq!(missing, fonts.system());
}

#[test]
fn shapes_and_caches_words() {
    let mut shaper = Shaper::default();
    let st = style(&mut shaper, "Helvetica");
    let hello = u16s("Hello");
    let a = shaper.shape_word(&hello, &st, false);
    assert_eq!(a.glyphs.len(), 5);
    assert!(a.advance > 20.0 && a.advance < 60.0, "{}", a.advance);
    assert!(a.glyphs.iter().all(|g| g.face == st.face && g.id != 0));
    assert_eq!(
        a.glyphs.iter().map(|g| g.cluster).collect::<Vec<_>>(),
        [0, 1, 2, 3, 4]
    );
    let b = shaper.shape_word(&hello, &st, false);
    assert!(Arc::ptr_eq(&a, &b));
    let bigger = Style { size: 32.0, ..st };
    let c = shaper.shape_word(&hello, &bigger, false);
    assert!(
        (c.advance - 2.0 * a.advance).abs() < 1.0,
        "{} {}",
        c.advance,
        a.advance
    );
    let s = shaper.cache.stats();
    assert_eq!((s.hits, s.misses, s.entries), (1, 2, 2));
    assert!((s.hit_rate() - 1.0 / 3.0).abs() < 1e-9);
}

#[test]
fn falls_back_by_script() {
    let mut shaper = Shaper::default();
    let st = style(&mut shaper, "Helvetica");
    let shaped = shaper.shape_word(&u16s("ab日本"), &st, false);
    let faces: Vec<FaceId> = shaped.glyphs.iter().map(|g| g.face).collect();
    assert_eq!(faces.len(), 4);
    assert_eq!(&faces[..2], &[st.face, st.face]);
    assert_ne!(faces[2], st.face);
    assert_eq!(faces[2], faces[3]);
    assert!(!shaper.fonts.family_name(faces[2]).is_empty());
    assert!(shaped.glyphs.iter().all(|g| g.id != 0));
    assert_eq!(shaper.fonts.fallback_stats().0, 1);
}

#[test]
fn right_to_left_glyphs_are_in_visual_order() {
    let mut shaper = Shaper::default();
    let st = style(&mut shaper, "Arial Hebrew");
    let shaped = shaper.shape_word(&u16s("\u{5E9}\u{5DC}\u{5D5}\u{5DD}"), &st, true);
    let clusters: Vec<u32> = shaped.glyphs.iter().map(|g| g.cluster).collect();
    assert_eq!(clusters, [3, 2, 1, 0]);
    assert!(shaped.rtl);
}

#[test]
fn cache_is_bounded_and_evicts_one_entry_at_a_time() {
    let mut shaper = Shaper::new(8, 1 << 20);
    let st = style(&mut shaper, "Helvetica");
    let words: Vec<Vec<u16>> = (0..50).map(|i| u16s(&format!("w{i}"))).collect();
    for w in &words {
        shaper.shape_word(w, &st, false);
    }
    let s = shaper.cache.stats();
    assert_eq!((s.entries, s.evictions, s.misses), (8, 42, 50));
    shaper.shape_word(&words[49], &st, false);
    shaper.shape_word(&words[0], &st, false);
    let s = shaper.cache.stats();
    assert_eq!((s.hits, s.misses, s.entries, s.evictions), (1, 51, 8, 43));
    // the weight bound: glyphs plus code units, each word at most a sixteenth of it
    let mut small = Shaper::new(1000, 96);
    let st = style(&mut small, "Helvetica");
    for w in &words {
        small.shape_word(w, &st, false);
    }
    let s = small.cache.stats();
    assert!(
        s.weight <= 96 && s.entries >= 16 && s.evictions > 0,
        "{s:?}"
    );
}

#[test]
fn breaks_lines_greedily() {
    let mut shaper = Shaper::default();
    let st = style(&mut shaper, "Helvetica");
    let text = u16s("the quick brown fox jumps\nover");
    let spans = [Span {
        range: 0..text.len(),
        style: st,
    }];
    let mut para = Paragraph::new();
    para.shape(&mut shaper, &text, &spans, Direction::Auto);
    let segs = para.segments();
    assert_eq!(segs.first().map(|s| s.range.start), Some(0));
    assert!(segs.windows(2).all(|w| w[0].range.end == w[1].range.start));
    assert_eq!(segs.last().map(|s| s.range.end), Some(text.len()));
    let mut lines = Vec::new();
    para.break_lines(10_000.0, &mut lines);
    assert_eq!(lines.len(), 2, "{lines:?}");
    let word = para.segments()[0].advance();
    para.break_lines(word * 2.5, &mut lines);
    assert!(lines.len() >= 3, "{lines:?}");
    for l in &lines {
        let words = para.segments()[l.segments.clone()]
            .iter()
            .filter(|s| s.kind == Kind::Text)
            .count();
        assert!(l.width <= word * 2.5 || words == 1, "{l:?}");
    }
    para.break_lines(1.0, &mut lines);
    assert_eq!(lines.len(), 6);
    // white space hangs at the end of a line: "the " is as wide as "the"
    assert!((lines[0].width - word).abs() < 0.01);
}

#[test]
fn orders_mixed_direction_lines() {
    let mut shaper = Shaper::default();
    let st = style(&mut shaper, "Helvetica");
    let text = u16s("abc \u{5D0}\u{5D1} \u{5D2}\u{5D3} def");
    let spans = [Span {
        range: 0..text.len(),
        style: st,
    }];
    let mut para = Paragraph::new();
    para.shape(&mut shaper, &text, &spans, Direction::Ltr);
    let segs: Vec<(usize, u8)> = para
        .segments()
        .iter()
        .map(|s| (s.range.start, s.level))
        .collect();
    assert_eq!(
        segs,
        [(0, 0), (3, 0), (4, 1), (6, 1), (7, 1), (9, 0), (10, 0)]
    );
    let mut lines = Vec::new();
    para.break_lines(10_000.0, &mut lines);
    let mut order = Vec::new();
    para.visual_order(&lines[0], &mut order);
    assert_eq!(order, [0, 1, 4, 3, 2, 5, 6]);
}

// every line fits the width unless it holds a single word
fn assert_lines_fit(para: &Paragraph, width: f32, lines: &[Line]) {
    for l in lines {
        let (mut words, mut text) = (0, false);
        for s in &para.segments()[l.segments.clone()] {
            text |= s.kind == Kind::Text;
            if s.after != After::Join {
                words += text as usize;
                text = false;
            }
        }
        words += text as usize;
        assert!(l.width <= width || words <= 1, "width {width} {l:?}");
    }
}

#[test]
fn lines_fit_once_hanging_space_becomes_inner() {
    let mut shaper = Shaper::default();
    let st = style(&mut shaper, "Helvetica");
    let mut para = Paragraph::new();
    let mut lines = Vec::new();
    for text in [
        "aaa bbb ccc",
        "the quick  brown fox   jumps over the lazy dog",
    ] {
        let text = u16s(text);
        let spans = [Span {
            range: 0..text.len(),
            style: st,
        }];
        para.shape(&mut shaper, &text, &spans, Direction::Ltr);
        para.break_lines(57.34, &mut lines);
        assert_lines_fit(&para, 57.34, &lines);
        for w in (1..400).map(|i| i as f32 * 0.5) {
            para.break_lines(w, &mut lines);
            assert_lines_fit(&para, w, &lines);
        }
    }
}

#[test]
fn empty_spans_give_no_segments() {
    let mut shaper = Shaper::default();
    let mut para = Paragraph::new();
    para.shape(&mut shaper, &u16s("abc def"), &[], Direction::Auto);
    assert!(para.segments().is_empty());
    let mut lines = vec![Line {
        segments: 0..1,
        width: 1.0,
    }];
    para.break_lines(100.0, &mut lines);
    assert!(lines.is_empty());
}

#[test]
fn match_cache_compares_the_whole_key() {
    let mut fonts = Fonts::new();
    let helvetica = fonts.select(&["Helvetica"], 400.0, false);
    assert_eq!(fonts.family_name(helvetica), "Helvetica");
    // the same 64-bit hash: the hasher pads the last chunk of a string with zeros
    let other = fonts.select(&["Helvetica\0"], 400.0, false);
    assert_ne!(fonts.system(), helvetica);
    assert_eq!(other, fonts.system());
}

#[test]
fn paragraph_buffers_shrink_after_a_long_text() {
    let mut shaper = Shaper::default();
    let st = style(&mut shaper, "Helvetica");
    let long = u16s(&"ab ".repeat(40_000));
    let mut para = Paragraph::new();
    for text in [long, u16s("ab")] {
        let spans = [Span {
            range: 0..text.len(),
            style: st,
        }];
        para.shape(&mut shaper, &text, &spans, Direction::Auto);
    }
    assert!(para.held() <= crate::para::KEEP, "{}", para.held());
}
