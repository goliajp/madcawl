//! `shot`: the first screen or the whole page painted offscreen, and the command writing a PNG.

use madcawl_shell::load::{self, Sheets};
use madcawl_shell::shot::{self, Options};

fn shoot(html: &str, o: Options) -> shot::Shot {
    let (dom, e) = load::parse(html.as_bytes(), None);
    let styles = load::style(&dom, None, e, &mut Sheets::default());
    shot::shot(&dom, &styles, o)
}

const PAGE: &str = "<body style='margin:0'><div style='height:2000px;background:rgb(0,255,0)'>\
                    <p style='margin:0;padding-top:1500px'>far down</p></div>";

#[test]
fn first_screen_is_the_viewport_at_scale() {
    let s = shoot(
        PAGE,
        Options {
            scale: 2.0,
            full: false,
        },
    );
    assert_eq!((s.bitmap.width, s.bitmap.height), (2560, 1600));
    assert_eq!(s.bitmap.pixel(100, 100), [0, 255, 0, 255]);
    assert_eq!(s.page_height, 2000.0);
    // the text below the first screen is not in the list
    assert_eq!(s.glyphs, 0);
    let one = shoot(
        PAGE,
        Options {
            scale: 1.0,
            full: false,
        },
    );
    assert_eq!((one.bitmap.width, one.bitmap.height), (1280, 800));
}

#[test]
fn full_page_reaches_the_lowest_fragment() {
    let s = shoot(
        PAGE,
        Options {
            scale: 1.0,
            full: true,
        },
    );
    assert_eq!((s.bitmap.width, s.bitmap.height), (1280, 2000));
    assert!(s.glyphs > 0, "the text far down is painted");
    assert_eq!(s.bitmap.pixel(10, 1990), [0, 255, 0, 255]);
}

#[test]
fn command_writes_a_png() {
    let dir = std::env::temp_dir().join(format!("madcawl-shot-{}", std::process::id()));
    std::fs::create_dir_all(&dir).expect("a scratch directory");
    let page = dir.join("p.html");
    let out = dir.join("p.png");
    std::fs::write(&page, PAGE).expect("the page");
    let r = std::process::Command::new(env!("CARGO_BIN_EXE_madcawl-shell"))
        .args([
            "shot",
            page.to_str().unwrap(),
            "-o",
            out.to_str().unwrap(),
            "--scale",
            "1",
        ])
        .output()
        .expect("the command runs");
    assert!(r.status.success(), "{}", String::from_utf8_lossy(&r.stderr));
    let png = std::fs::read(&out).expect("the PNG is written");
    assert_eq!(&png[..8], b"\x89PNG\r\n\x1a\n");
    // width and height in the IHDR chunk
    assert_eq!(&png[16..24], &[0, 0, 5, 0, 0, 0, 3, 32]);
    let bad = std::process::Command::new(env!("CARGO_BIN_EXE_madcawl-shell"))
        .args(["shot", page.to_str().unwrap()])
        .output()
        .expect("the command runs");
    assert_eq!(bad.status.code(), Some(2), "no -o is a usage error");
    std::fs::remove_dir_all(&dir).expect("the scratch directory goes");
}
