//! TextEncoder and TextDecoder on JavaScriptCore: UTF-8 of a USVString
//! into a new or a given Uint8Array, and decoding from any buffer source
//! in the encodings the standard names, with byte order marks, the fatal
//! error mode and streaming.

use madcawl_bindings::DomHost;
use madcawl_dom::Dom;
use madcawl_js::{Engine, ScriptOrigin};
use madcawl_js_jsc::Jsc;
use madcawl_str::DomString;

fn page() -> (Jsc, DomHost<Jsc>) {
    let mut e = Jsc::new();
    let mut h = DomHost::new(&mut e, Dom::new());
    h.install_window(&mut e);
    h.use_virtual_time();
    h.parse_document(&mut e, b"<body>");
    (e, h)
}

/// The result of `src` as a string, or what it threw.
fn eval(e: &mut Jsc, h: &mut DomHost<Jsc>, src: &str) -> String {
    let url = DomString::from_wtf8(b"text-coding");
    let realm = h.realm;
    let src = format!("String((function () {{ try {{ {src} }} catch (x) {{ return 'threw ' + x.name; }} }})())");
    let r = e.evaluate(h, realm, &DomString::from_wtf8(src.as_bytes()), &ScriptOrigin { url: &url, line: 1, column: 0 });
    assert!(!Jsc::is_thrown(r.get()), "{src} threw");
    let mut b = Vec::new();
    e.to_string(h, r.get()).expect("a string").write_wtf8(&mut b);
    e.release(r);
    h.settle(e);
    String::from_utf8(b).unwrap()
}

#[test]
fn text_encoder_encodes_usv_strings_as_utf8() {
    let (mut e, mut h) = page();
    let src = "var t = new TextEncoder(), a = t.encode('a\\u00e9\\ud83d\\ude00'), lone = t.encode('x\\ud800y');
return [t.encoding, a instanceof Uint8Array, a.length, Array.from(a).join(' '), Array.from(lone).join(' '), t.encode().length,
  t.encode(undefined).length, t.encode(null).length, Object.prototype.toString.call(t), TextEncoder.length].join();";
    assert_eq!(eval(&mut e, &mut h, src), "utf-8,true,7,97 195 169 240 159 152 128,120 239 191 189 121,0,0,4,[object TextEncoder],0");
    h.close(&mut e);
}

#[test]
fn encode_into_writes_whole_scalar_values_that_fit() {
    let (mut e, mut h) = page();
    let src = "var t = new TextEncoder(), b = new Uint8Array(5), r = t.encodeInto('a\\u00e9\\ud83d\\ude00', b);
var big = new Uint8Array(8), r2 = t.encodeInto('a\\u00e9\\ud83d\\ude00', big.subarray(1));
function name(f) { try { f(); return 'none'; } catch (x) { return x.name; } }
return [r.read, r.written, Array.from(b).join(' '), r2.read, r2.written, big[0], big[1],
  name(() => t.encodeInto('a', new Int8Array(2))), name(() => t.encodeInto('a', [0])), name(() => t.encodeInto('a'))].join();";
    assert_eq!(eval(&mut e, &mut h, src), "2,3,97 195 169 0 0,4,7,0,97,TypeError,TypeError,TypeError");
    h.close(&mut e);
}

#[test]
fn text_decoder_decodes_buffer_sources_and_drops_one_byte_order_mark() {
    let (mut e, mut h) = page();
    let src = "var d = new TextDecoder(), bytes = [0xEF, 0xBB, 0xBF, 0x61, 0xC3, 0xA9, 0xFF];
var u8 = new Uint8Array(bytes);
return [d.encoding, d.fatal, d.ignoreBOM, d.decode(u8), d.decode(u8.buffer), d.decode(new DataView(u8.buffer, 3, 3)),
  d.decode(new Uint16Array([0x6261])), d.decode(), d.decode(undefined),
  new TextDecoder('utf-8', { ignoreBOM: true }).decode(u8).length, new TextDecoder(' UTF8 ').encoding].join();";
    assert_eq!(eval(&mut e, &mut h, src), "utf-8,false,false,a\u{e9}\u{fffd},a\u{e9}\u{fffd},a\u{e9},ab,,,4,utf-8");
    h.close(&mut e);
}

#[test]
fn text_decoder_knows_the_standard_s_encodings_and_refuses_others() {
    let (mut e, mut h) = page();
    let src = "function name(f) { try { f(); return 'none'; } catch (x) { return x.name; } }
return [new TextDecoder('latin1').encoding, new TextDecoder('utf-16').encoding, new TextDecoder('shift_jis').encoding,
  new TextDecoder('windows-1252').decode(new Uint8Array([0x80, 0x41])),
  new TextDecoder('utf-16le').decode(new Uint8Array([0xFF, 0xFE, 0x41, 0x00, 0x3D, 0xD8, 0x00, 0xDE])),
  new TextDecoder('utf-16be').decode(new Uint8Array([0x00, 0x42])),
  new TextDecoder('shift_jis').decode(new Uint8Array([0x82, 0xA0])),
  name(() => new TextDecoder('replacement')), name(() => new TextDecoder('nope')), name(() => new TextDecoder('utf-8', 1)),
  name(() => new TextDecoder().decode('abc')), name(() => new TextDecoder().decode({}))].join();";
    let want = "windows-1252,utf-16le,shift_jis,\u{20ac}A,A\u{1f600},B,\u{3042},RangeError,RangeError,TypeError,TypeError,TypeError";
    assert_eq!(eval(&mut e, &mut h, src), want);
    h.close(&mut e);
}

#[test]
fn a_fatal_decoder_throws_at_the_first_error_and_starts_again() {
    let (mut e, mut h) = page();
    let src = "var d = new TextDecoder('utf-8', { fatal: true }), log = [d.fatal];
function step(f) { try { log.push(f()); } catch (x) { log.push(x.name); } }
step(() => d.decode(new Uint8Array([0x61, 0xFF])));
step(() => d.decode(new Uint8Array([0x62])));
step(() => d.decode(new Uint8Array([0xE2, 0x82]), { stream: true }));
step(() => d.decode(new Uint8Array([0xAC])));
step(() => d.decode(new Uint8Array([0xE2, 0x82])));
step(() => d.decode(new Uint8Array([0xED, 0xA0, 0x80])));
step(() => new TextDecoder('utf-16le', { fatal: true }).decode(new Uint8Array([0x00, 0xD8])));
step(() => new TextDecoder('shift_jis', { fatal: true }).decode(new Uint8Array([0x82, 0xFF])));
return log.join();";
    assert_eq!(eval(&mut e, &mut h, src), "true,TypeError,b,,\u{20ac},TypeError,TypeError,TypeError,TypeError");
    h.close(&mut e);
}

#[test]
fn streaming_keeps_a_cut_sequence_and_the_byte_order_mark_state() {
    let (mut e, mut h) = page();
    let src = "var d = new TextDecoder(), out = [];
for (var b of [0xEF, 0xBB, 0xBF, 0xEF, 0xBB, 0xBF, 0xF0, 0x9F, 0x98, 0x80]) out.push(d.decode(new Uint8Array([b]), { stream: true }));
out.push(d.decode());
var cut = d.decode(new Uint8Array([0xF0, 0x9F]), { stream: true }) + '|' + d.decode();
var bom = d.decode(new Uint8Array([0xEF, 0xBB]), { stream: true }) + '|' + d.decode(new Uint8Array([0xBF, 0x41]));
return [out.join('').length, out.join('').charCodeAt(0).toString(16), cut, bom].join();";
    assert_eq!(eval(&mut e, &mut h, src), "3,feff,|\u{fffd},|A");
    h.close(&mut e);
}

#[test]
fn an_iframe_s_encoder_makes_arrays_of_its_own_realm_which_script_may_transfer() {
    let (mut e, mut h) = page();
    let src = "var w = document.body.appendChild(document.createElement('iframe')).contentWindow, t = new w.TextEncoder();
var a = t.encode('ab'), r = t.encodeInto('c', new Uint8Array(2)), b = t.encode('xyz').buffer, moved = b.transfer();
return [a instanceof w.Uint8Array, a instanceof Uint8Array, Object.getPrototypeOf(r) === w.Object.prototype, t.encode('').length,
  b.byteLength, moved.byteLength, Array.from(new Uint8Array(moved)).join(' ')].join();";
    assert_eq!(eval(&mut e, &mut h, src), "true,false,true,0,0,3,120 121 122");
    h.close(&mut e);
}

#[test]
fn an_empty_string_encodes_to_an_empty_buffer_script_may_transfer_and_the_collector_frees() {
    let (mut e, mut h) = page();
    let src = "var t = new TextEncoder(), out = [];
for (var i = 0; i < 3; i++) {
  var a = t.encode(''), moved = a.buffer.transfer();
  out.push(a instanceof Uint8Array, a.length, moved.byteLength, a.buffer.byteLength, new Uint8Array(moved).length);
}
return out.join();";
    assert_eq!(eval(&mut e, &mut h, src), "true,0,0,0,0,".repeat(3).trim_end_matches(','));
    e.collect_now();
    assert_eq!(eval(&mut e, &mut h, "return new TextEncoder().encode('').byteOffset;"), "0");
    h.close(&mut e);
}

#[test]
fn methods_of_a_gone_iframe_s_window_throw_instead_of_making_values_elsewhere() {
    let (mut e, mut h) = page();
    let src = "var f = document.body.appendChild(document.createElement('iframe')), w = f.contentWindow;
var t = new w.TextEncoder(), b = new w.Blob(['x']), p = new w.URLSearchParams('a=1');
f.remove();
function name(g) { try { g(); return 'none'; } catch (x) { return x.name; } }
return [name(() => t.encode('a')), name(() => t.encodeInto('a', new Uint8Array(1))), name(() => b.text()),
  name(() => b.arrayBuffer()), name(() => p.getAll('a')), name(() => p.entries()), name(() => p.get('a'))].join();";
    assert_eq!(eval(&mut e, &mut h, src), "TypeError,TypeError,TypeError,TypeError,TypeError,TypeError,none");
    h.close(&mut e);
}
