//! Fitting a view's lines into a budget of estimated tokens by folding
//! structure, never cutting a line: first the items of long lists and
//! tables past the first few, then whole subtrees from the deepest level
//! up, each fold written as one line `… N more [ref]` that the reference
//! expands; only then the tail.

use crate::text::estimate_tokens;

/// One line of a view, before indentation.
pub(crate) struct Line {
    pub depth: u16,
    pub text: String,
    /// The reference that expands a fold of this line's children
    pub reference: Option<String>,
    /// Its children are items of a list, a table, a menu
    pub many: bool,
}

/// List items kept before the rest are folded.
const KEEP: usize = 3;

fn cost(l: &Line) -> usize {
    estimate_tokens(&l.text) + l.depth as usize / 2 + 1
}

/// The end (exclusive) of each line's subtree.
fn ends(lines: &[Line]) -> Vec<usize> {
    let mut end = vec![lines.len(); lines.len()];
    let mut open: Vec<usize> = Vec::new();
    for (i, l) in lines.iter().enumerate() {
        while let Some(&top) = open.last() {
            if lines[top].depth >= l.depth {
                end[top] = i;
                open.pop();
            } else {
                break;
            }
        }
        open.push(i);
    }
    end
}

fn summary(depth: u16, n: usize, reference: &Option<String>) -> Line {
    let mut text = format!("… {n} more");
    if let Some(r) = reference {
        text.push_str(" [");
        text.push_str(r);
        text.push(']');
    }
    Line {
        depth,
        text,
        reference: None,
        many: false,
    }
}

/// Folds the children of the lines `pick` chooses: of a chosen line,
/// its direct children from the `keep`-th on and everything below them
/// become one summary line.
fn fold_where(lines: Vec<Line>, keep: usize, pick: impl Fn(&Line) -> bool) -> (Vec<Line>, usize) {
    let end = ends(&lines);
    let mut out = Vec::with_capacity(lines.len());
    let mut folded = 0;
    let mut pending: Vec<(usize, Line)> = Vec::new();
    let mut lines: Vec<Option<Line>> = lines.into_iter().map(Some).collect();
    // (depth, reference) of the open lines that have one, for a fold of
    // a line without its own
    let mut refs: Vec<(u16, String)> = Vec::new();
    for i in 0..lines.len() {
        while pending.last().is_some_and(|(at, _)| *at <= i) {
            out.push(pending.pop().expect("checked").1);
        }
        let Some(l) = lines[i].take() else { continue };
        while refs.last().is_some_and(|(d, _)| *d >= l.depth) {
            refs.pop();
        }
        if pick(&l) && end[i] > i + 1 {
            let mut child = i + 1;
            let mut k = 0;
            while child < end[i] && k < keep {
                child = end[child];
                k += 1;
            }
            if child < end[i] {
                let mut items = 0;
                let mut c = child;
                while c < end[i] {
                    items += 1;
                    c = end[c];
                }
                folded += end[i] - child;
                let reference = l
                    .reference
                    .clone()
                    .or_else(|| refs.last().map(|(_, r)| r.clone()));
                pending.push((end[i], summary(l.depth + 1, items, &reference)));
                // the children past `keep` are skipped once reached
                for slot in lines.iter_mut().take(end[i]).skip(child) {
                    *slot = None;
                }
            }
        }
        if let Some(r) = &l.reference {
            refs.push((l.depth, r.clone()));
        }
        out.push(l);
    }
    while let Some((_, s)) = pending.pop() {
        out.push(s);
    }
    (out, folded)
}

/// The lines, folded to `budget` when one is given, and how many lines
/// were folded away.
pub(crate) fn fit(mut lines: Vec<Line>, budget: Option<usize>) -> (Vec<Line>, usize) {
    let Some(budget) = budget else {
        return (lines, 0);
    };
    let total = |ls: &[Line]| ls.iter().map(cost).sum::<usize>();
    let mut folded = 0;
    if total(&lines) > budget {
        let (l, f) = fold_where(lines, KEEP, |l| l.many);
        lines = l;
        folded += f;
    }
    let max_depth = lines.iter().map(|l| l.depth).max().unwrap_or(0);
    let mut d = max_depth;
    while total(&lines) > budget && d > 0 {
        d -= 1;
        let (l, f) = fold_where(lines, 0, |l| l.depth == d);
        lines = l;
        folded += f;
    }
    if total(&lines) > budget {
        let tail = |rest: usize| Line {
            depth: 0,
            text: format!("… {rest} more lines past the budget; read a smaller scope"),
            reference: None,
            many: false,
        };
        // the tail line's own cost, at the largest count it can say
        let reserve = cost(&tail(lines.len()));
        let mut used = 0;
        let mut cut = lines.len();
        for (i, l) in lines.iter().enumerate() {
            used += cost(l);
            if used + reserve > budget {
                cut = i;
                break;
            }
        }
        let rest = lines.len() - cut;
        lines.truncate(cut);
        folded += rest;
        lines.push(tail(rest));
    }
    (lines, folded)
}

/// The lines, indented two spaces a level, one per line.
pub(crate) fn join(lines: &[Line]) -> String {
    let mut out = String::new();
    for l in lines {
        for _ in 0..l.depth {
            out.push_str("  ");
        }
        out.push_str(&l.text);
        out.push('\n');
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn line(depth: u16, text: &str, many: bool) -> Line {
        Line {
            depth,
            text: text.to_owned(),
            reference: Some(format!("r{text}")),
            many,
        }
    }

    #[test]
    fn long_lists_fold_past_three_items_with_the_list_reference() {
        let mut lines = vec![line(0, "list", true)];
        for i in 0..10 {
            lines.push(line(1, &format!("item{i}"), false));
            lines.push(line(2, "text", false));
        }
        lines.push(line(0, "after", false));
        let (out, folded) = fit(lines, Some(30));
        let text = join(&out);
        assert!(text.contains("item2"), "{text}");
        assert!(!text.contains("item3"), "{text}");
        assert!(text.contains("  … 7 more [rlist]\nafter"), "{text}");
        assert_eq!(folded, 14);
    }

    #[test]
    fn the_tail_fits_the_budget_and_folds_take_an_ancestor_reference() {
        let mut lines = vec![line(0, "root", false)];
        lines.push(Line {
            depth: 1,
            text: "plain".into(),
            reference: None,
            many: true,
        });
        for i in 0..10 {
            lines.push(line(2, &format!("item{i}"), false));
        }
        let (out, _) = fold_where(lines, KEEP, |l| l.many);
        assert!(
            join(&out).contains("    … 7 more [rroot]"),
            "{}",
            join(&out)
        );
        let long: Vec<Line> = (0..200)
            .map(|i| line(0, &format!("a fairly long line of text number {i}"), false))
            .collect();
        for budget in [20, 33, 50, 100] {
            let (out, _) = fit(
                long.iter().map(|l| line(l.depth, &l.text, false)).collect(),
                Some(budget),
            );
            let total: usize = out.iter().map(cost).sum();
            assert!(total <= budget, "{total} > {budget}");
        }
    }

    #[test]
    fn deep_subtrees_fold_before_the_tail_is_cut() {
        let mut lines = Vec::new();
        for i in 0..4 {
            lines.push(line(0, &format!("section{i}"), false));
            for _ in 0..20 {
                lines.push(line(1, "a long line of text in the section", false));
            }
        }
        let (out, _) = fit(lines, Some(40));
        let text = join(&out);
        assert!(text.contains("section3"), "{text}");
        assert!(text.contains("  … 20 more [rsection0]"), "{text}");
    }
}
