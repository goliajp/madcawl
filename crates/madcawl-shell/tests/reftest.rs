//! `reftest`: pages that paint the same pass `==`, pages that differ pass `!=`, a sheet named from the
//! root is read from below `--root`, and a missing page is an error, one result line per case. Fuzzy
//! allowances and several references are judged as wptrunner judges them, in an 800px wide viewport.

use std::io::Write as _;
use std::process::{Command, Stdio};

fn run(dir: &std::path::Path, cases: &str) -> Vec<Vec<String>> {
    let mut c = Command::new(env!("CARGO_BIN_EXE_madcawl-shell"))
        .args(["reftest", "--root", dir.to_str().unwrap()])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .spawn()
        .expect("the command runs");
    c.stdin.take().unwrap().write_all(cases.as_bytes()).unwrap();
    let out = c.wait_with_output().unwrap();
    assert!(out.status.success());
    String::from_utf8(out.stdout)
        .unwrap()
        .lines()
        .map(|l| l.split('\t').map(str::to_owned).collect())
        .collect()
}

#[test]
fn reference_pages_are_compared_pixel_by_pixel() {
    let dir = std::env::temp_dir().join(format!("madcawl-reftest-{}", std::process::id()));
    std::fs::create_dir_all(dir.join("support")).unwrap();
    let put = |name: &str, html: &str| std::fs::write(dir.join(name), html).unwrap();
    std::fs::write(
        dir.join("support/green.css"),
        ".box{background:rgb(0,128,0)}",
    )
    .unwrap();
    put(
        "t.html",
        "<body style='margin:0'><div style='margin-left:20px;width:50px;height:50px;background:green'></div>",
    );
    put(
        "r.html",
        "<body style='margin:0'><div style='padding-left:20px'><div style='width:50px;height:50px;background:rgb(0,128,0)'></div></div>",
    );
    put(
        "rel.html",
        "<link rel=stylesheet href='/support/green.css'><body style='margin:0'><div class=box style='margin-left:20px;width:50px;height:50px'></div>",
    );
    put("blank.html", "<body>");
    put(
        "media.html",
        "<style>div{background:rgb(0,128,0)} @media (min-width: 801px) {div{background:red}}</style><body style='margin:0'><div style='margin-left:20px;width:50px;height:50px'></div>",
    );
    put(
        "other.html",
        "<body style='margin:0'><div style='width:50px;height:50px;background:red'></div>",
    );
    let p = |n: &str| dir.join(n).display().to_string();
    let cases = format!(
        "same\t{t}\t0\t0\t==\t{r}\nroot\t{rel}\t0\t0\t==\t{r}\ndiffer\t{t}\t0\t0\t!=\t{o}\nwrong\t{t}\t0\t0\t==\t{o}\nfuzzy\t{t}\t0-255\t0-5000\t==\t{o}\nmissing\t{m}\t0\t0\t==\t{r}\nblank\t{b}\t0\t0\t==\t{b}\nfuzzy-ne\t{t}\t0-255\t0-5000\t!=\t{o}\nany\t{t}\t0\t0\t==\t{o}\t==\t{r}\nall-ne\t{t}\t0\t0\t==\t{r}\t!=\t{r}\nfloor\t{t}\t1-255\t1-5000\t==\t{r}\nviewport\t{md}\t0\t0\t==\t{r}\n",
        t = p("t.html"),
        r = p("r.html"),
        rel = p("rel.html"),
        o = p("other.html"),
        m = p("nope.html"),
        b = p("blank.html"),
        md = p("media.html"),
    );
    let got = run(&dir, &cases);
    let status: Vec<(&str, &str)> = got.iter().map(|l| (l[0].as_str(), l[1].as_str())).collect();
    assert_eq!(
        status,
        [
            ("same", "pass"),
            ("root", "pass"),
            ("differ", "pass"),
            ("wrong", "fail"),
            ("fuzzy", "pass"),
            ("missing", "error"),
            ("blank", "pass"),
            // a fuzzy allowance makes the differing pages equal, so `!=` fails
            ("fuzzy-ne", "fail"),
            // one `==` reference matching is enough
            ("any", "pass"),
            // every `!=` reference must differ
            ("all-ne", "fail"),
            // identical pages fail when both ranges start above 0
            ("floor", "fail"),
            // styled for the 800px viewport the page is laid out in
            ("viewport", "pass"),
        ]
    );
    // a page of one color is noted, so a pass on it is told apart
    assert_eq!(got[0][5], "");
    assert_eq!(got[6][5], "uniform");
    // the wrong reference differs where the two boxes do not overlap, by a full channel
    assert_eq!(got[3][3], "255");
    assert!(got[3][2].parse::<u64>().unwrap() > 1000);
    std::fs::remove_dir_all(&dir).unwrap();
}
