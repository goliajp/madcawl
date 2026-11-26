//! A case line of the `reftest` command and the fuzzy allowance it
//! carries.

use std::path::PathBuf;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Fuzzy {
    pub max_diff: (u8, u8),
    pub total: (u64, u64),
}

impl Fuzzy {
    pub const EXACT: Fuzzy = Fuzzy {
        max_diff: (0, 0),
        total: (0, 0),
    };

    /// Whether a difference of `pixels` pixels, the largest channel
    /// difference `max`, counts as equal, as wptrunner decides it: with
    /// no allowance only identical pictures are; with one, both numbers
    /// in their ranges, or a number that is 0 where its range starts at
    /// 0. So identical pictures are not equal when both ranges start
    /// above 0.
    pub fn equal(&self, pixels: u64, max: u8) -> bool {
        if *self == Fuzzy::EXACT {
            return pixels == 0 && max == 0;
        }
        (pixels == 0 && self.total.0 == 0)
            || (max == 0 && self.max_diff.0 == 0)
            || ((self.max_diff.0..=self.max_diff.1).contains(&max)
                && (self.total.0..=self.total.1).contains(&pixels))
    }
}

fn range<T: std::str::FromStr + Copy>(s: &str) -> Option<(T, T)> {
    match s.split_once('-') {
        Some((a, b)) => Some((a.trim().parse().ok()?, b.trim().parse().ok()?)),
        None => s.trim().parse().ok().map(|v| (v, v)),
    }
}

pub struct Case {
    pub id: String,
    pub test: PathBuf,
    pub fuzzy: Fuzzy,
    /// Each reference and whether it must match (`==`) or differ (`!=`)
    pub refs: Vec<(bool, PathBuf)>,
}

impl Case {
    pub fn parse(line: &str) -> Option<Case> {
        let mut f = line.split('\t');
        let (id, test) = (f.next()?.to_owned(), PathBuf::from(f.next()?));
        let fuzzy = Fuzzy {
            max_diff: range(f.next()?)?,
            total: range(f.next()?)?,
        };
        let mut refs = Vec::new();
        while let Some(op) = f.next() {
            let to = PathBuf::from(f.next()?);
            refs.push((
                match op {
                    "==" => true,
                    "!=" => false,
                    _ => return None,
                },
                to,
            ));
        }
        (!refs.is_empty()).then_some(Case {
            id,
            test,
            fuzzy,
            refs,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn case_lines_and_fuzzy_ranges_are_read() {
        let c = Case::parse("a/t.html\t/w/a/t.html\t0-2\t0-300\t==\t/w/a/r.html\t!=\t/w/a/n.html")
            .unwrap();
        assert_eq!(c.id, "a/t.html");
        assert_eq!(
            c.fuzzy,
            Fuzzy {
                max_diff: (0, 2),
                total: (0, 300)
            }
        );
        assert_eq!(
            c.refs,
            vec![
                (true, PathBuf::from("/w/a/r.html")),
                (false, PathBuf::from("/w/a/n.html"))
            ]
        );
        assert!(Case::parse("a\t/t\t0\t0").is_none(), "no reference");
        assert!(Case::parse("a\t/t\t0\t0\t~=\t/r").is_none());
        assert!(Fuzzy::EXACT.equal(0, 0));
        assert!(!Fuzzy::EXACT.equal(1, 1));
        assert!(c.fuzzy.equal(300, 2));
        assert!(!c.fuzzy.equal(301, 2));
        assert!(!c.fuzzy.equal(10, 3));
        assert_eq!(
            Case::parse("a\t/t\t5\t7\t==\t/r").unwrap().fuzzy.total,
            (7, 7)
        );
    }

    #[test]
    fn fuzzy_ranges_are_judged_as_wptrunner_judges_them() {
        let f = |d: (u8, u8), t: (u64, u64)| Fuzzy {
            max_diff: d,
            total: t,
        };
        // identical pictures fail only when both ranges start above 0
        assert!(!f((1, 2), (1, 10)).equal(0, 0));
        assert!(f((1, 2), (0, 10)).equal(0, 0));
        assert!(f((0, 2), (5, 10)).equal(0, 0));
        // otherwise both numbers must be in range, ends included
        assert!(f((1, 2), (1, 10)).equal(10, 1));
        assert!(!f((1, 2), (1, 10)).equal(11, 1));
        assert!(
            !f((3, 9), (0, 10)).equal(5, 2),
            "under the smallest difference allowed"
        );
        assert!(!f((0, 0), (0, 0)).equal(1, 1));
    }
}
