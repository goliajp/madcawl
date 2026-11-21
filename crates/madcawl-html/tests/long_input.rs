//! One document of about 1 MB made of the inputs of the tree
//! construction tests, fed whole, a byte at a time and in random chunks
//! of up to 8 KiB: the three trees must be the same. The test inputs are
//! all under 1.1 KB, so this is what makes the tokenizer move its input
//! buffer forward (it does so once more than 4 KiB and more than half of
//! it has been read).

use madcawl_html::{Options, Tokenizer, TreeBuilder};

#[allow(dead_code)]
#[path = "support/dom.rs"]
mod dom;
#[allow(dead_code)]
#[path = "support/wpt.rs"]
mod wpt;

use dom::{Dom, print};
use wpt::{Feed, feed_all, parse_tests};

const TARGET: usize = 1 << 20;

fn document() -> Vec<u8> {
    let dir =
        std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/data/wpt-tree-construction");
    let mut files: Vec<_> = std::fs::read_dir(&dir)
        .unwrap()
        .map(|e| e.unwrap().path())
        .filter(|p| p.extension().is_some_and(|e| e == "dat"))
        .collect();
    files.sort();
    let mut parts = Vec::new();
    for path in &files {
        for t in parse_tests(&std::fs::read(path).unwrap()) {
            // scripted cases need a script engine; plaintext would turn
            // everything after it into one text node
            let lower = t.data.to_ascii_lowercase();
            if t.fragment.is_none()
                && t.script != Some(true)
                && !lower.windows(9).any(|w| w == b"plaintext")
            {
                parts.push(t.data);
            }
        }
    }
    assert!(parts.len() > 1000, "only {} inputs", parts.len());
    let mut doc = Vec::with_capacity(TARGET + 2048);
    while doc.len() < TARGET {
        for p in &parts {
            doc.extend_from_slice(p);
            doc.push(b'\n');
        }
    }
    doc
}

fn tree(doc: &[u8], feed: Feed) -> Vec<u8> {
    let mut tok = Tokenizer::new();
    let mut b = TreeBuilder::new(Dom::new(), Options::default());
    feed_all(&mut tok, &mut b, doc, feed);
    let dom = b.into_sink();
    let mut out = Vec::new();
    print(&dom, 0, 0, &mut out);
    out
}

#[test]
fn a_long_document_parses_the_same_however_it_is_fed() {
    let doc = document();
    let whole = tree(&doc, Feed::Whole);
    eprintln!("{} bytes in, {} bytes of tree out", doc.len(), whole.len());
    assert!(
        whole.len() > doc.len() / 4,
        "the tree is too small to mean much"
    );
    assert!(
        whole == tree(&doc, Feed::Bytewise),
        "byte at a time differs from whole"
    );
    let chunks = Feed::Chunks {
        seed: 0x6c6f_6e67,
        max: 8192,
    };
    assert!(
        whole == tree(&doc, chunks),
        "random chunks differ from whole"
    );
}
