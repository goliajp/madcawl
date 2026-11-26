//! `madcawl-shell reftest`: reference tests, a test page and its
//! reference pages each painted offscreen and compared pixel by pixel.
//! Cases come one a line on stdin, tab-separated: an id, the test file,
//! the fuzzy allowance (`maxDifference` and `totalPixels`, each `lo-hi`
//! or one number), then pairs of `==` or `!=` and a reference file. A
//! result goes to stdout a line, written as soon as the case is judged:
//! the id, `pass`, `fail` or `error`, the pixels that differ and the
//! largest channel difference against the reference that decided it,
//! the milliseconds the case took, and a note: `uniform` when the test
//! page painted one color only, so a pass says little, or why the case
//! could not run. A case that crashes the process ends the run; the
//! caller starts again after the last id answered.
//!
//! A case passes as WPT judges it: one of its `==` references, if it has
//! any, matches, and every `!=` reference differs, matching under the
//! fuzzy allowance in both cases (see [`Fuzzy::equal`]); colors are
//! compared without alpha.
//!
//! Pages are styled and laid out in an 800x600 viewport at scale 1, as
//! WPT renders reference tests. A path in a stylesheet `href` that
//! starts with `/` is read from below `--root`, and with `--root` no
//! sheet outside it is read; `--font` registers a font file for this
//! process only, as the suites expect their test fonts installed. A
//! `.xht` or `.xhtml` file gets what an XML parser would change in it
//! first, as far as `xhtml` goes; there is no XML parser.

use std::collections::HashMap;
use std::io::{BufRead, Write};
use std::path::{Component, Path, PathBuf};
use std::time::Instant;

use madcawl_css::decode_stylesheet;
use madcawl_css::style::{Device, style_document};
use madcawl_layout::Layout;
use madcawl_paint::{Bitmap, Raster, RectF};

use crate::load;

mod case;
mod font;
mod xhtml;

pub use case::{Case, Fuzzy};
pub use font::register_font;

pub const VIEWPORT: (f32, f32) = (800.0, 600.0);
/// Reference pictures kept for later cases that share a reference.
const KEPT: usize = 32;

/// The pixels of `a` and `b` that differ in color, and the largest
/// channel difference; alpha is left out, as wptrunner compares RGB.
/// Pictures of another size differ everywhere.
pub fn compare(a: &Bitmap, b: &Bitmap) -> (u64, u8) {
    if (a.width, a.height) != (b.width, b.height) {
        return ((a.width * a.height).max(b.width * b.height) as u64, 255);
    }
    let (mut n, mut max) = (0u64, 0u8);
    for (p, q) in a
        .data
        .as_chunks::<4>()
        .0
        .iter()
        .zip(b.data.as_chunks::<4>().0)
    {
        let d = p[..3]
            .iter()
            .zip(&q[..3])
            .map(|(x, y)| x.abs_diff(*y))
            .max()
            .unwrap_or(0);
        if d > 0 {
            n += 1;
            max = max.max(d);
        }
    }
    (n, max)
}

/// `path` painted in the reference test viewport.
pub fn render(path: &Path, root: Option<&Path>) -> std::io::Result<Bitmap> {
    let mut bytes = load::read_file(path, load::MAX_FILE)?;
    if path.extension().is_some_and(|e| e == "xht" || e == "xhtml") {
        bytes = xhtml::as_html(&String::from_utf8_lossy(&bytes)).into_bytes();
    }
    let (dom, encoding) = load::parse(&bytes, None);
    let base = path.parent().unwrap_or(Path::new("/"));
    let (w, h) = VIEWPORT;
    let device = Device {
        width: w as f64,
        height: h as f64,
        ..Device::default()
    };
    let styles = style_document(&dom, device, &mut |href| {
        let file = href.split(['?', '#']).next().unwrap_or("");
        let p = match (file.strip_prefix('/'), root) {
            (Some(rest), Some(r)) => r.join(rest),
            (Some(_), None) => return None,
            _ if file.contains(':') || file.is_empty() => return None,
            _ => base.join(file),
        };
        if root.is_some_and(|r| !within(&p, r)) {
            return None;
        }
        let b = load::read_file(&p, load::MAX_FILE).ok()?;
        Some(decode_stylesheet(&b, None, Some(encoding)).0)
    });
    let mut layout = Layout::new(&dom, &styles, w, h);
    let view = RectF {
        x: 0.0,
        y: 0.0,
        w,
        h,
    };
    let list = madcawl_paint::paint(&mut layout, &dom, &styles, Some(view));
    Ok(Raster { view, scale: 1.0 }.draw(&list, &mut layout.shaper().fonts))
}

/// `p` with `.` and `..` resolved as written, links not looked at.
fn lexical(p: &Path) -> PathBuf {
    let mut out = PathBuf::new();
    for c in p.components() {
        match c {
            Component::CurDir => {}
            Component::ParentDir => {
                out.pop();
            }
            c => out.push(c),
        }
    }
    out
}

/// Whether `p` stays below `root` once its `..` are resolved.
fn within(p: &Path, root: &Path) -> bool {
    lexical(p).starts_with(lexical(root))
}

/// The command: `[--root <dir>] [--font <file>]...`, cases on stdin,
/// results on stdout. Err(None) for arguments it does not take.
pub fn command(args: &[&str]) -> Result<(), Option<String>> {
    let mut root = None;
    let mut it = args.iter();
    while let Some(&opt) = it.next() {
        match (opt, it.next()) {
            ("--root", Some(&d)) => root = Some(Path::new(d)),
            ("--font", Some(&f)) if register_font(Path::new(f)) => {}
            ("--font", Some(f)) => return Err(Some(format!("cannot register the font {f}"))),
            _ => return Err(None),
        }
    }
    run(std::io::stdin().lock(), std::io::stdout().lock(), root).map_err(|e| Some(e.to_string()))
}

/// Runs the cases read from `input`, a result line each to `out`.
pub fn run(input: impl BufRead, mut out: impl Write, root: Option<&Path>) -> std::io::Result<()> {
    let mut kept: HashMap<PathBuf, Bitmap> = HashMap::new();
    for line in input.lines() {
        let line = line?;
        let Some(c) = Case::parse(&line) else {
            if !line.trim().is_empty() {
                writeln!(
                    out,
                    "{}\terror\t\t\t\tnot a case line",
                    line.split('\t').next().unwrap_or("")
                )?;
            }
            continue;
        };
        let t = Instant::now();
        let r = judge(&c, root, &mut kept);
        let ms = t.elapsed().as_secs_f64() * 1e3;
        match r {
            Ok((ok, n, max, flat)) => {
                let (verdict, note) = (
                    if ok { "pass" } else { "fail" },
                    if flat { "uniform" } else { "" },
                );
                writeln!(out, "{}\t{verdict}\t{n}\t{max}\t{ms:.1}\t{note}", c.id)?
            }
            Err(e) => writeln!(out, "{}\terror\t\t\t{ms:.1}\t{e}", c.id)?,
        }
        out.flush()?;
    }
    Ok(())
}

/// Whether every pixel of `b` is the same.
pub fn uniform(b: &Bitmap) -> bool {
    let px = b.data.as_chunks::<4>().0;
    px.iter().all(|p| p == &px[0])
}

/// `path` painted, from `kept` or into it.
fn picture<'k>(
    kept: &'k mut HashMap<PathBuf, Bitmap>,
    path: &Path,
    root: Option<&Path>,
) -> std::io::Result<&'k Bitmap> {
    if !kept.contains_key(path) {
        if kept.len() >= KEPT {
            kept.clear();
        }
        kept.insert(path.to_path_buf(), render(path, root)?);
    }
    Ok(&kept[path])
}

/// Whether case `c` passes, with the difference that decided it and
/// whether the test page painted one color only. A `==` reference left
/// after one matched is not painted.
fn judge(
    c: &Case,
    root: Option<&Path>,
    kept: &mut HashMap<PathBuf, Bitmap>,
) -> std::io::Result<(bool, u64, u8, bool)> {
    let test = render(&c.test, root)?;
    let flat = uniform(&test);
    let (mut matched, mut missed, mut differed) = (None, None, (0, 0));
    for (must_match, path) in &c.refs {
        if *must_match && matched.is_some() {
            continue;
        }
        let (n, max) = compare(&test, picture(kept, path, root)?);
        match (*must_match, c.fuzzy.equal(n, max)) {
            (false, true) => return Ok((false, n, max, flat)),
            (false, false) => differed = (n, max),
            (true, true) => matched = Some((n, max)),
            (true, false) => {
                missed.get_or_insert((n, max));
            }
        }
    }
    let (ok, (n, max)) = match (matched, missed) {
        (Some(d), _) => (true, d),
        (None, Some(d)) => (false, d),
        (None, None) => (true, differed),
    };
    Ok((ok, n, max, flat))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sheets_outside_the_root_are_not_read() {
        let root = Path::new("/w/root");
        assert!(within(Path::new("/w/root/a/../b.css"), root));
        assert!(!within(Path::new("/w/root/../etc/x.css"), root));
        assert!(!within(Path::new("/w/root/a/../../x.css"), root));
        assert!(!within(Path::new("/w/rootx/a.css"), root));
    }
}
