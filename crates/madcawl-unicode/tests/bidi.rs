//! The UAX #9 conformance data: BidiTest (classes only) and BidiCharacterTest (code points, with brackets).

mod support;

use madcawl_unicode::{Bidi, BidiClass, Direction, Utf32};
use support::{data, hex, report};

fn class(name: &str) -> BidiClass {
    use BidiClass::*;
    match name {
        "L" => L,
        "R" => R,
        "AL" => AL,
        "EN" => EN,
        "ES" => ES,
        "ET" => ET,
        "AN" => AN,
        "CS" => CS,
        "NSM" => NSM,
        "BN" => BN,
        "B" => B,
        "S" => S,
        "WS" => WS,
        "ON" => ON,
        "LRE" => LRE,
        "LRO" => LRO,
        "RLE" => RLE,
        "RLO" => RLO,
        "PDF" => PDF,
        "LRI" => LRI,
        "RLI" => RLI,
        "FSI" => FSI,
        "PDI" => PDI,
        _ => panic!("unknown bidi class {name}"),
    }
}

struct Check {
    bidi: Bidi,
    levels: Vec<u8>,
    order: Vec<usize>,
    scratch: Vec<u8>,
}

impl Check {
    // the levels (None for characters removed by X9) and the visual order without them, of one line holding
    // the whole paragraph
    fn line(&mut self) -> (Vec<Option<u8>>, Vec<usize>) {
        let n = self.bidi.len();
        self.bidi.line_levels(0..n, &mut self.levels);
        Bidi::reorder(&self.levels, 0, &mut self.order, &mut self.scratch);
        let levels = (0..n)
            .map(|i| (!self.bidi.is_removed(i)).then_some(self.levels[i]))
            .collect();
        let order = self
            .order
            .iter()
            .copied()
            .filter(|&i| !self.bidi.is_removed(i))
            .collect();
        (levels, order)
    }
}

fn parse_levels(s: &str) -> Vec<Option<u8>> {
    s.split_whitespace()
        .map(|t| {
            if t == "x" {
                None
            } else {
                Some(t.parse().unwrap_or_else(|e| panic!("level {t}: {e}")))
            }
        })
        .collect()
}

fn parse_order(s: &str) -> Vec<usize> {
    s.split_whitespace()
        .map(|t| t.parse().unwrap_or_else(|e| panic!("index {t}: {e}")))
        .collect()
}

#[test]
fn bidi_test() {
    let mut c = Check {
        bidi: Bidi::new(),
        levels: Vec::new(),
        order: Vec::new(),
        scratch: Vec::new(),
    };
    let (mut want_levels, mut want_order) = (Vec::new(), Vec::new());
    let (mut pass, mut total, mut failures) = (0, 0, Vec::new());
    for (n, line) in data("BidiTest.txt").lines().enumerate() {
        let line = line.split('#').next().unwrap_or("").trim();
        if let Some(v) = line.strip_prefix("@Levels:") {
            want_levels = parse_levels(v);
            continue;
        }
        if let Some(v) = line.strip_prefix("@Reorder:") {
            want_order = parse_order(v);
            continue;
        }
        if line.is_empty() {
            continue;
        }
        let Some((classes, bits)) = line.split_once(';') else {
            panic!("BidiTest.txt:{}: no ';' in {line:?}", n + 1)
        };
        let classes: Vec<BidiClass> = classes.split_whitespace().map(class).collect();
        let bits: u8 = bits
            .trim()
            .parse()
            .unwrap_or_else(|e| panic!("BidiTest.txt:{}: {e}", n + 1));
        for (bit, dir) in [
            (1, Direction::Auto),
            (2, Direction::Ltr),
            (4, Direction::Rtl),
        ] {
            if bits & bit == 0 {
                continue;
            }
            total += 1;
            c.bidi.resolve_classes(&classes, dir);
            let (levels, order) = c.line();
            if levels == want_levels && order == want_order {
                pass += 1;
            } else {
                failures.push(format!(
                    "BidiTest.txt:{} {dir:?} {classes:?} levels {levels:?} order {order:?}",
                    n + 1
                ));
            }
        }
    }
    report("BidiTest", pass, total, 770_241, &failures);
}

#[test]
fn bidi_character_test() {
    let mut c = Check {
        bidi: Bidi::new(),
        levels: Vec::new(),
        order: Vec::new(),
        scratch: Vec::new(),
    };
    let (mut pass, mut total, mut failures) = (0, 0, Vec::new());
    for (n, line) in data("BidiCharacterTest.txt").lines().enumerate() {
        let line = line.split('#').next().unwrap_or("").trim();
        if line.is_empty() {
            continue;
        }
        let f: Vec<&str> = line.split(';').collect();
        assert_eq!(f.len(), 5, "BidiCharacterTest.txt:{}: {line:?}", n + 1);
        total += 1;
        let cps: Vec<u32> = f[0].split_whitespace().map(hex).collect();
        let dir = match f[1].trim() {
            "0" => Direction::Ltr,
            "1" => Direction::Rtl,
            "2" => Direction::Auto,
            d => panic!("BidiCharacterTest.txt:{}: direction {d}", n + 1),
        };
        c.bidi.resolve(&Utf32(&cps), dir);
        let para = c.bidi.paragraphs().first().map(|p| p.level);
        let (levels, order) = c.line();
        let want: u8 = f[2]
            .trim()
            .parse()
            .unwrap_or_else(|e| panic!("BidiCharacterTest.txt:{}: {e}", n + 1));
        let ok = para == Some(want) && levels == parse_levels(f[3]) && order == parse_order(f[4]);
        if ok {
            pass += 1;
        } else {
            failures.push(format!(
                "BidiCharacterTest.txt:{} para {para:?} levels {levels:?} order {order:?}",
                n + 1
            ));
        }
    }
    report("BidiCharacterTest", pass, total, 91_707, &failures);
}
