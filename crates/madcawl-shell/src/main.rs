//! `madcawl-shell read <file|->` reads a local page (no scripts, no
//! network) as text an agent can read, with how long each stage took
//! and the peak memory; `madcawl-shell audit <file|->` reads it and
//! checks that nothing hidden made it into the text; `madcawl-shell shot
//! <file|-> -o <out.png>` lays it out in a 1280x800 viewport and paints
//! the first screen (`--full`: the whole page) offscreen at device scale
//! 2 (`--scale 1` for 1); `madcawl-shell --serve <socket>` serves
//! windowless pages to agents over a Unix socket; `madcawl-shell fetch
//! <url> -o <file>` saves a page fetched over the network.

use std::io::{Read as _, Write as _};
use std::path::Path;
use std::process::ExitCode;
use std::time::{Duration, Instant};

use madcawl_dom::{Dom, NodeKind};
use madcawl_html::Encoding;
use madcawl_shell::load::{self, Sheets};
use madcawl_shell::{Read, Shown, audit, read, rusage, serve, shot};

const USAGE: &str = "usage: madcawl-shell read <file|-> [--charset <label>] [--stats <file>]
       madcawl-shell audit <file|-> [--charset <label>]
       madcawl-shell shot <file|-> -o <out.png> [--charset <label>] [--scale 1|2] [--full]
       madcawl-shell fetch <url> -o <file>
       ";

/// What the command line asked for.
struct Args<'a> {
    cmd: &'a str,
    path: &'a str,
    charset: Option<Encoding>,
    stats: Option<&'a str>,
    out: Option<&'a str>,
    scale: f32,
    full: bool,
}

fn parse_args<'a>(args: &[&'a str]) -> Option<Args<'a>> {
    let [cmd, path, rest @ ..] = args else {
        return None;
    };
    let mut a = Args {
        cmd,
        path,
        charset: None,
        stats: None,
        out: None,
        scale: 2.0,
        full: false,
    };
    let mut rest = rest.iter();
    while let Some(&opt) = rest.next() {
        if opt == "--full" && *cmd == "shot" {
            a.full = true;
            continue;
        }
        let v = *rest.next()?;
        match opt {
            "--charset" => a.charset = Some(Encoding::for_label(v.as_bytes())?),
            "--stats" if *cmd == "read" => a.stats = Some(v),
            "-o" if *cmd == "shot" => a.out = Some(v),
            "--scale" if *cmd == "shot" && matches!(v, "1" | "2") => a.scale = v.parse().ok()?,
            _ => return None,
        }
    }
    let ok = matches!(*cmd, "read" | "audit") || *cmd == "shot" && a.out.is_some();
    ok.then_some(a)
}

struct Run {
    bytes: usize,
    encoding: String,
    sheets: Sheets,
    times: [(&'static str, Duration); 4],
}

fn input(path: &str) -> std::io::Result<Vec<u8>> {
    if path == "-" {
        let mut v = Vec::new();
        std::io::stdin().read_to_end(&mut v)?;
        return Ok(v);
    }
    std::fs::read(path)
}

/// Parse, style, read and write `path`; calls `then` with what was
/// built, before it is dropped.
fn run<R>(
    path: &str,
    charset: Option<Encoding>,
    then: impl FnOnce(&Dom, &Shown, &Read, &str, Run) -> R,
) -> std::io::Result<R> {
    let t = Instant::now();
    let bytes = input(path)?;
    let t_read = t.elapsed();
    let t = Instant::now();
    let (dom, encoding) = load::parse(&bytes, charset);
    let t_parse = t.elapsed();
    let t = Instant::now();
    let base = (path != "-").then(|| Path::new(path).parent().unwrap_or(Path::new(".")));
    let mut sheets = Sheets::default();
    let styles = load::style(&dom, base, encoding, &mut sheets);
    let t_style = t.elapsed();
    let t = Instant::now();
    let shown = Shown::new(&dom, &styles);
    let r = read(&shown);
    let mut text = String::new();
    madcawl_shell::read::write_text(&r, path, &mut text);
    let t_extract = t.elapsed();
    let run = Run {
        bytes: bytes.len(),
        encoding: format!("{encoding:?}"),
        sheets,
        times: [
            ("read", t_read),
            ("parse", t_parse),
            ("style", t_style),
            ("extract", t_extract),
        ],
    };
    Ok(then(&dom, &shown, &r, &text, run))
}

fn ms(d: Duration) -> String {
    format!("{:.2}", d.as_secs_f64() * 1e3)
}

fn stats(dom: &Dom, shown: &Shown, r: &Read, text: &str, run: &Run) -> String {
    let peak = rusage::peak_rss();
    let (mut elements, mut texts, mut hidden_el, mut hidden_text) = (0, 0, 0, 0);
    for n in dom.descendants(dom.document()) {
        match dom.kind(n) {
            NodeKind::Element => {
                elements += 1;
                hidden_el += usize::from(!shown.shown(n));
            }
            NodeKind::Text => {
                texts += 1;
                hidden_text += usize::from(!shown.shown(n));
            }
            _ => {}
        }
    }
    let mut out = String::new();
    let mut put = |k: &str, v: String| out.push_str(&format!("{k} {v}\n"));
    put("bytes", run.bytes.to_string());
    put("encoding", run.encoding.clone());
    let parse_to_text: Duration = run.times[1..].iter().map(|t| t.1).sum();
    for (k, d) in run.times {
        put(&format!("time_ms.{k}"), ms(d));
    }
    put("time_ms.parse_style_extract", ms(parse_to_text));
    put("peak_rss_bytes", peak.to_string());
    put(
        "peak_rss_mib",
        format!("{:.1}", peak as f64 / (1 << 20) as f64),
    );
    put("dom_bytes", dom.bytes().to_string());
    put("nodes", dom.node_count().to_string());
    put("elements", elements.to_string());
    put("text_nodes", texts.to_string());
    put("hidden_elements", hidden_el.to_string());
    put("hidden_text_nodes", hidden_text.to_string());
    put("sheets.inline", load::count_inline(dom).to_string());
    put("sheets.linked_loaded", run.sheets.linked.to_string());
    put(
        "sheets.linked_missing",
        run.sheets.missing.len().to_string(),
    );
    put("items", r.items.len().to_string());
    put("output_bytes", text.len().to_string());
    out
}

fn cmd_read(a: &Args) -> std::io::Result<()> {
    let s = run(a.path, a.charset, |dom, shown, r, text, run| {
        std::io::stdout().write_all(text.as_bytes())?;
        Ok::<_, std::io::Error>(stats(dom, shown, r, text, &run))
    })??;
    match a.stats {
        Some(p) => std::fs::write(p, s),
        None => std::io::stderr().write_all(s.as_bytes()),
    }
}

fn cmd_audit(a: &Args) -> std::io::Result<bool> {
    run(a.path, a.charset, |dom, shown, r, text, _| {
        let a = audit::audit(dom, shown.styles, r, text);
        println!(
            "hidden nodes: {} elements, {} text",
            a.hidden_elements, a.hidden_texts
        );
        println!(
            "by node: {} nodes read, {} of them hidden {:?}",
            a.used,
            a.used_hidden.len(),
            &a.used_hidden[..a.used_hidden.len().min(8)]
        );
        println!(
            "by reference: {} references in the text, {} of them hidden {:?}",
            a.refs,
            a.refs_hidden.len(),
            &a.refs_hidden[..a.refs_hidden.len().min(8)]
        );
        println!(
            "by text: {} hidden pieces, {} sampled, {} also shown elsewhere, {} searched, {} found {:?}",
            a.hidden_pieces,
            a.hidden_sampled,
            a.not_distinct,
            a.hidden_tested,
            a.hidden_found.len(),
            &a.hidden_found[..a.hidden_found.len().min(4)]
        );
        println!(
            "control: {} shown pieces searched, {} found",
            a.shown_tested, a.shown_found
        );
        // with nothing shown the checks have nothing to catch
        let verdict = match a.verdict() {
            audit::Verdict::Pass => "PASS",
            audit::Verdict::Empty => "EMPTY (nothing shown; no hidden node leaked)",
            audit::Verdict::Fail => "FAIL",
        };
        println!("{verdict}");
        a.verdict() != audit::Verdict::Fail
    })
}

fn cmd_shot(a: &Args) -> std::io::Result<()> {
    let bytes = input(a.path)?;
    let (dom, encoding) = load::parse(&bytes, a.charset);
    let base = (a.path != "-").then(|| Path::new(a.path).parent().unwrap_or(Path::new(".")));
    let device = madcawl_css::style::Device {
        scale: a.scale as f64,
        ..Default::default()
    };
    let styles = load::style_for(&dom, base, encoding, &mut Sheets::default(), device);
    let o = shot::Options {
        scale: a.scale,
        full: a.full,
    };
    let s = shot::shot(&dom, &styles, o);
    shot::write(&s, Path::new(a.out.unwrap_or("out.png")))?;
    let mut e = format!(
        "size {}x{}\npage_height_px {:.0}\nentries {}\nglyphs {}\nculled {}\ndropped {}\n",
        s.bitmap.width, s.bitmap.height, s.page_height, s.entries, s.glyphs, s.culled, s.dropped
    );
    for (k, d) in s.times {
        e.push_str(&format!("time_ms.{k} {}\n", ms(d)));
    }
    std::io::stderr().write_all(e.as_bytes())
}

fn cmd_serve(args: &[&str]) -> ExitCode {
    let Some(c) = serve::parse_args(args) else {
        eprintln!("{USAGE}{}", serve::USAGE);
        return ExitCode::from(2);
    };
    match serve::Server::bind(c).and_then(serve::Server::run) {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("madcawl-shell: {e}");
            ExitCode::FAILURE
        }
    }
}

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let args: Vec<&str> = args.iter().map(String::as_str).collect();
    if let ["--serve", rest @ ..] = args.as_slice() {
        return cmd_serve(rest);
    }
    if let ["fetch", url, "-o", out] = args.as_slice() {
        return match madcawl_shell::fetch::command(url, out) {
            Ok(()) => ExitCode::SUCCESS,
            Err(e) => {
                eprintln!("madcawl-shell: {e}");
                ExitCode::FAILURE
            }
        };
    }
    let Some(a) = parse_args(&args) else {
        eprintln!("{USAGE}{}", serve::USAGE);
        return ExitCode::from(2);
    };
    let done = match a.cmd {
        "read" => cmd_read(&a).map(|_| true),
        "shot" => cmd_shot(&a).map(|_| true),
        _ => cmd_audit(&a),
    };
    match done {
        Ok(true) => ExitCode::SUCCESS,
        Ok(false) => ExitCode::FAILURE,
        Err(e) => {
            eprintln!("madcawl-shell: {e}");
            ExitCode::FAILURE
        }
    }
}
