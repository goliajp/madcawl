//! Seeded mutations of the corpus: byte edits, pieces of markup the
//! parser treats specially, splices of two inputs, and repeats that
//! grow an input so work that is not linear in its length shows.

use crate::wpt::Rng;

const MAX_LEN: usize = 1 << 16;

const PIECES: &[&[u8]] = &[
    b"<",
    b">",
    b"</",
    b"/>",
    b"&",
    b"&amp;",
    b"&#",
    b"&#x",
    b";",
    b"=",
    b"\"",
    b"'",
    b"-",
    b"--",
    b"<!--",
    b"-->",
    b"<!",
    b"<?",
    b"<![CDATA[",
    b"]]>",
    b"\0",
    b"\r",
    b"\r\n",
    b"\xef\xbf\xbd",
    b"\xe2\x82",
    b"\xf0\x9f",
    b"\xed\xa0\x80",
    b"<html>",
    b"<head>",
    b"<body>",
    b"<table>",
    b"<tr>",
    b"<td>",
    b"</table>",
    b"<b>",
    b"</b>",
    b"<a href=x>",
    b"</a>",
    b"<p>",
    b"</p>",
    b"<div>",
    b"</div>",
    b"<template>",
    b"</template>",
    b"<svg>",
    b"</svg>",
    b"<math>",
    b"<mi>",
    b"<foreignObject>",
    b"<select>",
    b"<option>",
    b"<selectedcontent>",
    b"<form>",
    b"<input>",
    b"<textarea>",
    b"<script>",
    b"</script>",
    b"<style>",
    b"<title>",
    b"<plaintext>",
    b"<frameset>",
    b"<noscript>",
    b"<iframe>",
    b"<xmp>",
    b"<!DOCTYPE html>",
    b"<button>",
    b"<li>",
    b"<dd>",
    b"<h1>",
    b"<ruby>",
    b"<rt>",
    b"<nobr>",
    b"<font color=1>",
];

fn below(rng: &mut Rng, n: usize) -> usize {
    if n == 0 {
        0
    } else {
        (rng.next() % n as u64) as usize
    }
}

pub fn input(corpus: &[Vec<u8>], rng: &mut Rng) -> Vec<u8> {
    let mut v = corpus[below(rng, corpus.len())].clone();
    for _ in 0..1 + below(rng, 8) {
        let at = below(rng, v.len() + 1);
        match rng.next() % 8 {
            0 if !v.is_empty() => {
                let i = below(rng, v.len());
                v[i] ^= 1 << (rng.next() % 8);
            }
            1 => v.insert(at, rng.next() as u8),
            2 if !v.is_empty() => {
                let n = 1 + below(rng, 16.min(v.len() - at.min(v.len() - 1)));
                let at = at.min(v.len() - 1);
                v.drain(at..(at + n).min(v.len()));
            }
            3 | 4 => {
                let p = PIECES[below(rng, PIECES.len())];
                v.splice(at..at, p.iter().copied());
            }
            5 => {
                let other = &corpus[below(rng, corpus.len())];
                let from = below(rng, other.len() + 1);
                v.splice(at..at, other[from..].iter().copied());
            }
            6 if !v.is_empty() => {
                // a piece of the input many times over
                let from = below(rng, v.len());
                let n = 1 + below(rng, 32.min(v.len() - from));
                let piece = v[from..from + n].to_vec();
                let times = 1 + below(rng, 4096 / n);
                let run = piece.repeat(times);
                v.splice(at..at, run);
            }
            _ => {
                let p = PIECES[below(rng, PIECES.len())];
                let run = p.repeat(1 + below(rng, 1024));
                v.splice(at..at, run);
            }
        }
        v.truncate(MAX_LEN);
    }
    v
}
