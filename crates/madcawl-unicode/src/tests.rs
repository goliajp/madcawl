use crate::*;

#[test]
fn properties() {
    let a = props('a' as u32);
    assert_eq!(
        (a.bidi, a.line, a.script, a.word),
        (
            BidiClass::L,
            LineBreak::AL,
            Script::Latin,
            WordBreak::ALetter
        )
    );
    let alef = props(0x05D0);
    assert_eq!(
        (alef.bidi, alef.line, alef.script),
        (BidiClass::R, LineBreak::HL, Script::Hebrew)
    );
    assert_eq!(props(0x4E00).east_asian, EastAsianWidth::W);
    assert!(props(0x1F600).extended_pictographic());
    assert_eq!(props(0x094D).conjunct_break(), ConjunctBreak::Linker);
    // unassigned code points take the defaults of their block
    assert_eq!(props(0x05FF).bidi, BidiClass::R);
    assert_eq!(props(0x3FFFD).line, LineBreak::ID);
    assert_eq!(props(0x11_0000).category, GeneralCategory::Cn);
    assert_eq!(mirror('(' as u32), Some(')' as u32));
    assert_eq!(bracket(0x232A), Some((0x3008, false)));
}

#[test]
fn sources_agree() {
    let text = "a\u{300}\u{1F468}\u{200D}\u{1F469} b";
    let utf16: Vec<u16> = text.encode_utf16().collect();
    let utf32: Vec<u32> = text.chars().map(|c| c as u32).collect();
    let g8: Vec<usize> = grapheme_breaks(text).collect();
    assert_eq!(g8, [3, 14, 15, 16]);
    assert_eq!(grapheme_breaks(&Utf16(&utf16)).count(), 4);
    assert_eq!(
        grapheme_breaks(&Utf32(&utf32)).collect::<Vec<_>>(),
        [2, 5, 6, 7]
    );
    let lone = [0xD800u16, 0x61];
    assert_eq!(Utf16(&lone).next(0), (0xFFFD, 1));
    assert_eq!(Utf16(&lone).prev(1), (0xFFFD, 0));
}

#[test]
fn lines_and_words() {
    let text = "Hello, world.\nNext";
    let b: Vec<Break> = LineBreaks::new(text).collect();
    assert_eq!(
        b,
        [
            Break {
                pos: 7,
                mandatory: false
            },
            Break {
                pos: 14,
                mandatory: true
            },
            Break {
                pos: 18,
                mandatory: true
            }
        ]
    );
    assert_eq!(word_breaks("can't stop").collect::<Vec<_>>(), [5, 6, 10]);
    assert_eq!(LineBreaks::new("").count(), 0);
}

#[test]
fn bidi_levels_and_order() {
    let mut bidi = Bidi::new();
    let text = "abc \u{5D0}\u{5D1}\u{5D2} def";
    bidi.resolve(text, Direction::Auto);
    assert_eq!(bidi.paragraphs()[0].level, 0);
    assert_eq!(bidi.levels(), [0, 0, 0, 0, 1, 1, 1, 0, 0, 0, 0]);
    let (mut levels, mut order, mut scratch) = (Vec::new(), Vec::new(), Vec::new());
    bidi.line_levels(0..bidi.len(), &mut levels);
    Bidi::reorder(&levels, 0, &mut order, &mut scratch);
    assert_eq!(order, [0, 1, 2, 3, 6, 5, 4, 7, 8, 9, 10]);
    assert_eq!(bidi.offset(4), 4);
    assert_eq!(bidi.offset(5), 6);
}

#[test]
fn bidi_buffers_shrink_after_a_long_text() {
    let long: Vec<u32> = "ab ".repeat(40_000).chars().map(|c| c as u32).collect();
    let mut bidi = Bidi::new();
    bidi.resolve(&Utf32(&long), Direction::Auto);
    assert_eq!(bidi.len(), long.len());
    bidi.resolve(&Utf32(&[0x61]), Direction::Auto);
    assert_eq!(bidi.len(), 1);
    assert!(bidi.held() <= bidi::KEEP, "{}", bidi.held());
}
