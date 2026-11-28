//! XMLHttpRequest on JavaScriptCore: the requests it hands the embedder,
//! the events and state a response or a failure brings, the response
//! types, the checks on methods and headers, CORS mode, timeouts and
//! blob URLs.

use madcawl_bindings::{Credentials, DomHost, FetchRequest, FetchResponse, Mode};
use madcawl_dom::Dom;
use madcawl_js::{Engine, ScriptOrigin};
use madcawl_js_jsc::Jsc;
use madcawl_str::DomString;

fn page(virtual_time: bool) -> (Jsc, DomHost<Jsc>) {
    let mut e = Jsc::new();
    let mut h = DomHost::new(&mut e, Dom::new());
    assert!(h.set_url("https://a.test/dir/page"));
    h.install_window(&mut e);
    if virtual_time {
        h.use_virtual_time();
    }
    h.parse_document(&mut e, b"<body>");
    while h.run_tasks(&mut e, 100) != 0 {}
    (e, h)
}

/// The result of `src` as a string, or what it threw.
fn eval(e: &mut Jsc, h: &mut DomHost<Jsc>, src: &str) -> String {
    let url = DomString::from_wtf8(b"xhr");
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

fn response(status: u16, url: &str, headers: &[(&str, &str)], body: &[u8]) -> FetchResponse {
    FetchResponse {
        status,
        status_text: if status == 200 { "OK".into() } else { "Not Found".into() },
        url: url.into(),
        headers: headers.iter().map(|(n, v)| ((*n).to_owned(), v.as_bytes().to_vec())).collect(),
        body: body.to_vec(),
    }
}

fn run(e: &mut Jsc, h: &mut DomHost<Jsc>) {
    while h.run_tasks(e, 100) != 0 {}
}

/// Records every event of `x` in `log`, with the state it saw.
const WATCH: &str = "window.log = []; window.watch = function (x) {
  for (const t of ['readystatechange', 'loadstart', 'progress', 'load', 'error', 'abort', 'timeout', 'loadend'])
    x.addEventListener(t, ev => log.push(t + ':' + x.readyState + (ev.lengthComputable ? ':' + ev.loaded : '')));
};";

#[test]
fn a_request_goes_to_the_embedder_and_its_response_fires_the_events_in_order() {
    let (mut e, mut h) = page(true);
    let src = "window.x = new XMLHttpRequest(); watch(x); log.push(x.readyState); x.open('get', 'data?q=1#f'); x.setRequestHeader('X-A', ' 1 ');
x.setRequestHeader('x-a', '2'); x.setRequestHeader('Cookie', 'c=1'); x.send(); log.push('sent'); return log.join();";
    eval(&mut e, &mut h, WATCH);
    assert_eq!(eval(&mut e, &mut h, src), "0,readystatechange:1,loadstart:1,sent");
    let reqs = h.take_fetches();
    let want = FetchRequest {
        id: reqs[0].id,
        destination: "xhr",
        method: "GET".into(),
        url: "https://a.test/dir/data?q=1#f".into(),
        headers: vec![("X-A".into(), b"1, 2".to_vec())],
        body: None,
        initiator: Some("https://a.test/dir/page".into()),
        credentials: Credentials::SameOrigin,
        mode: Mode::Cors,
    };
    assert_eq!(reqs, [want]);
    assert_eq!(h.fetches_in_flight(), 1);
    let headers = [("Content-Type", "text/plain; charset=utf-8"), ("X-B", "b"), ("x-b", "c"), ("Set-Cookie", "s=1")];
    h.fetch_done(reqs[0].id, Some(response(200, "https://a.test/dir/data?q=1#f", &headers, "h\u{e9}llo".as_bytes())));
    run(&mut e, &mut h);
    let want = "0,readystatechange:1,loadstart:1,sent,readystatechange:2,readystatechange:3,progress:3:6,readystatechange:4,load:4:6,loadend:4:6";
    assert_eq!(eval(&mut e, &mut h, "return log.join();"), want);
    let read = "return [x.status, x.statusText, x.responseText, x.response, x.responseURL, x.getResponseHeader('X-b'),
  x.getResponseHeader('set-cookie'), JSON.stringify(x.getAllResponseHeaders())].join('|');";
    let want = "200|OK|h\u{e9}llo|h\u{e9}llo|https://a.test/dir/data?q=1|b, c||\"content-type: text/plain; charset=utf-8\\r\\nx-b: b, c\\r\\n\"";
    assert_eq!(eval(&mut e, &mut h, read), want);
    assert_eq!(h.fetches_in_flight(), 0);
    h.close(&mut e);
}

#[test]
fn response_types_make_their_object_once() {
    let (mut e, mut h) = page(true);
    let src = "window.xs = {}; for (const t of ['json', 'arraybuffer', 'blob', 'text', 'document', 'bad']) {
  const x = xs[t] = new XMLHttpRequest(); x.open('GET', '/r'); x.responseType = t; x.send(); }
xs.badjson = new XMLHttpRequest(); xs.badjson.open('GET', '/r'); xs.badjson.responseType = 'json'; xs.badjson.send();
return [xs.bad.responseType, xs.json.response, (() => { try { return xs.json.responseText; } catch (x) { return x.name; } })()].join();";
    assert_eq!(eval(&mut e, &mut h, src), ",,InvalidStateError");
    let reqs = h.take_fetches();
    assert_eq!(reqs.len(), 7);
    for r in &reqs[..6] {
        h.fetch_done(r.id, Some(response(200, &r.url, &[("Content-Type", "Text/JSON")], b"\xef\xbb\xbf{\"a\":[1,\"\xc3\xa9\"]}")));
    }
    h.fetch_done(reqs[6].id, Some(response(200, &reqs[6].url, &[], b"{nope")));
    run(&mut e, &mut h);
    let read = "const j = xs.json.response, b = xs.arraybuffer.response;
return [j.a[1], j === xs.json.response, b instanceof ArrayBuffer, b.byteLength, b === xs.arraybuffer.response,
  xs.blob.response instanceof Blob, xs.blob.response.type, xs.blob.response.size, xs.text.response.length, xs.document.response,
  xs.badjson.response, xs.badjson.status].join();";
    assert_eq!(eval(&mut e, &mut h, read), "\u{e9},true,true,17,true,true,text/json,17,13,,,200");
    eval(&mut e, &mut h, "window.out = ''; xs.blob.response.text().then(t => out = t);");
    run(&mut e, &mut h);
    assert_eq!(eval(&mut e, &mut h, "return out;"), "{\"a\":[1,\"\u{e9}\"]}");
    h.close(&mut e);
}

#[test]
fn a_network_error_and_abort_end_the_request() {
    let (mut e, mut h) = page(true);
    eval(&mut e, &mut h, WATCH);
    eval(&mut e, &mut h, "window.x = new XMLHttpRequest(); watch(x); x.open('GET', '/a'); x.send();");
    let id = h.take_fetches()[0].id;
    h.fetch_done(id, None);
    run(&mut e, &mut h);
    let got = eval(&mut e, &mut h, "return log.join() + '|' + x.status + '|' + x.readyState + '|' + JSON.stringify(x.getAllResponseHeaders());");
    assert_eq!(got, "readystatechange:1,loadstart:1,readystatechange:4,error:4,loadend:4|0|4|\"\"");
    eval(&mut e, &mut h, "log.length = 0; x.open('GET', '/b'); x.send();");
    let id = h.take_fetches()[0].id;
    eval(&mut e, &mut h, "x.abort();");
    h.fetch_done(id, Some(response(200, "https://a.test/b", &[], b"late")));
    run(&mut e, &mut h);
    let got = eval(&mut e, &mut h, "return log.join() + '|' + x.readyState + '|' + x.responseText;");
    assert_eq!(got, "readystatechange:1,loadstart:1,readystatechange:4,abort:4,loadend:4|0|", "a response after abort is dropped");
    assert_eq!(h.fetches_in_flight(), 0);
    h.close(&mut e);
}

#[test]
fn a_request_times_out_on_the_clock_and_waiting_holds_virtual_time() {
    let (mut e, mut h) = page(true);
    eval(&mut e, &mut h, WATCH);
    eval(&mut e, &mut h, "window.x = new XMLHttpRequest(); watch(x); x.open('GET', '/slow'); x.timeout = 20; x.send(); setTimeout(() => log.push('timer'), 5);");
    run(&mut e, &mut h);
    assert_eq!(eval(&mut e, &mut h, "return log.join();"), "readystatechange:1,loadstart:1", "virtual time waits while a request is awaited");
    drop(h.take_fetches());
    let (mut e2, mut h2) = page(false);
    eval(&mut e2, &mut h2, WATCH);
    eval(&mut e2, &mut h2, "window.x = new XMLHttpRequest(); watch(x); x.open('GET', '/slow'); x.timeout = 20; x.send(); x.timeout = 40;");
    let id = h2.take_fetches()[0].id;
    std::thread::sleep(std::time::Duration::from_millis(25));
    run(&mut e2, &mut h2);
    assert_eq!(eval(&mut e2, &mut h2, "return log.join();"), "readystatechange:1,loadstart:1", "a later timeout counts from send");
    std::thread::sleep(std::time::Duration::from_millis(20));
    run(&mut e2, &mut h2);
    assert_eq!(eval(&mut e2, &mut h2, "return log.join();"), "readystatechange:1,loadstart:1,readystatechange:4,timeout:4,loadend:4");
    h2.fetch_done(id, Some(response(200, "https://a.test/slow", &[], b"x")));
    run(&mut e2, &mut h2);
    assert_eq!(eval(&mut e2, &mut h2, "return x.readyState + ':' + x.status;"), "4:0");
    h.close(&mut e);
    h2.close(&mut e2);
}

#[test]
fn methods_headers_and_states_are_checked() {
    let (mut e, mut h) = page(true);
    let src = "function name(f) { try { f(); return 'none'; } catch (x) { return x.name; } }
const x = new XMLHttpRequest();
return [name(() => x.setRequestHeader('a', 'b')), name(() => x.send()), name(() => x.open('GET', '/', false)), name(() => x.open('TRACE', '/')),
  name(() => x.open('bad method', '/')), name(() => x.open('G\\u0100T', '/')), name(() => x.open('GET', 'http://[::1')),
  name(() => x.open('POST', '/', true)), name(() => x.setRequestHeader('bad name', 'v')), name(() => x.setRequestHeader('a', 'x\\ny')),
  name(() => x.setRequestHeader('a', '\\u0100')), name(() => { x.withCredentials = true; }), name(() => x.send('b')), name(() => x.send()),
  name(() => x.setRequestHeader('a', 'b')), name(() => { x.withCredentials = false; }), name(() => { x.responseType = 'json'; })].join();";
    let want = "InvalidStateError,InvalidStateError,InvalidAccessError,SecurityError,SyntaxError,TypeError,SyntaxError,none,SyntaxError,SyntaxError,TypeError,none,none,InvalidStateError,InvalidStateError,InvalidStateError,none";
    assert_eq!(eval(&mut e, &mut h, src), want);
    let reqs = h.take_fetches();
    assert_eq!((reqs.len(), reqs[0].method.as_str(), reqs[0].body.as_deref()), (1, "POST", Some(b"b".as_slice())));
    assert_eq!(reqs[0].headers, [("Content-Type".to_owned(), b"text/plain;charset=UTF-8".to_vec())]);
    h.close(&mut e);
}

#[test]
fn bodies_are_extracted_with_their_types() {
    let (mut e, mut h) = page(true);
    let src = "function post(body, type) { const x = new XMLHttpRequest(); x.open('PUT', '/p'); if (type) x.setRequestHeader('Content-Type', type); x.send(body); }
post(new URLSearchParams({ a: '1 2', b: '\\u00e9' })); post(new Blob(['bl'], { type: 'Text/X' })); post(new Uint8Array([1, 2, 3]).subarray(1));
post({ toString() { return 'obj'; } }, 'application/json'); post(null); post(new Blob(['n']));";
    eval(&mut e, &mut h, src);
    type Headers = Vec<(String, Vec<u8>)>;
    let got: Vec<(Option<Vec<u8>>, Headers)> = h.take_fetches().into_iter().map(|r| (r.body, r.headers)).collect();
    let ct = |v: &str| vec![("Content-Type".to_owned(), v.as_bytes().to_vec())];
    assert_eq!(got[0], (Some(b"a=1+2&b=%C3%A9".to_vec()), ct("application/x-www-form-urlencoded;charset=UTF-8")));
    assert_eq!(got[1], (Some(b"bl".to_vec()), ct("text/x")));
    assert_eq!(got[2], (Some(vec![2, 3]), vec![]));
    assert_eq!(got[3], (Some(b"obj".to_vec()), ct("application/json")));
    assert_eq!(got[4], (None, vec![]));
    assert_eq!(got[5], (Some(b"n".to_vec()), vec![]));
    h.close(&mut e);
}

#[test]
fn another_origin_is_read_only_past_the_cors_check() {
    let (mut e, mut h) = page(true);
    let src = "window.xs = []; function get(url, creds, header) { const x = new XMLHttpRequest(); x.open('GET', url); x.withCredentials = creds;
  if (header) x.setRequestHeader('X-Custom', '1'); x.onloadend = () => x.done = x.status + ':' + x.responseText; x.send(); xs.push(x); }
get('https://b.test/1', false); get('https://b.test/2', true); get('https://b.test/3', false); get('https://b.test/4', false, true);
get('/same-then-elsewhere', false);";
    eval(&mut e, &mut h, src);
    let reqs = h.take_fetches();
    let (same, include) = (Credentials::SameOrigin, Credentials::Include);
    assert_eq!(reqs.iter().map(|r| r.credentials).collect::<Vec<_>>(), [same, include, same, same], "a request that needs a preflight is not sent");
    let star = [("Access-Control-Allow-Origin", "*"), ("X-Hidden", "h"), ("X-Shown", "s"), ("Access-Control-Expose-Headers", "x-shown"), ("Content-Type", "text/plain")];
    h.fetch_done(reqs[0].id, Some(response(200, "https://b.test/1", &star, b"one")));
    h.fetch_done(reqs[1].id, Some(response(200, "https://b.test/2", &star, b"two")));
    h.fetch_done(reqs[2].id, Some(response(200, "https://b.test/3", &[], b"three")));
    h.fetch_done(reqs[3].id, Some(response(200, "https://c.test/x", &[("Access-Control-Allow-Origin", "https://a.test")], b"four")));
    run(&mut e, &mut h);
    let got = eval(&mut e, &mut h, "return xs.map(x => x.done).join('|') + '|' + JSON.stringify(xs[0].getAllResponseHeaders());");
    assert_eq!(got, "200:one|0:|0:|0:|200:four|\"content-type: text/plain\\r\\nx-shown: s\\r\\n\"");
    h.close(&mut e);
}

#[test]
fn a_blob_url_is_read_without_the_network() {
    let (mut e, mut h) = page(true);
    let src = "window.u = URL.createObjectURL(new Blob(['blob body'], { type: 'text/x' })); window.x = new XMLHttpRequest(); x.open('GET', u); x.send();
window.y = new XMLHttpRequest(); y.open('POST', u); y.send('z'); window.z = new XMLHttpRequest(); z.open('GET', 'data:,x'); z.send();";
    eval(&mut e, &mut h, src);
    assert!(h.take_fetches().is_empty());
    run(&mut e, &mut h);
    let got = eval(&mut e, &mut h, "return [x.status, x.responseText, x.getResponseHeader('content-type'), y.status, z.status, z.readyState].join();");
    assert_eq!(got, "200,blob body,text/x,0,0,4");
    h.close(&mut e);
}

#[test]
fn a_waiting_request_keeps_its_object_and_requests_are_bounded() {
    let (mut e, mut h) = page(true);
    eval(&mut e, &mut h, "window.done = 0; (() => { const x = new XMLHttpRequest(); x.open('GET', '/k'); x.onload = () => done = x.responseText; x.send(); })();");
    let id = h.take_fetches()[0].id;
    let pad = [0u8; 512 * 1024];
    std::hint::black_box(&pad);
    e.collect_now();
    while h.collect(&mut e, madcawl_js::Deadline::Work(u32::MAX)) != madcawl_js::Step::Done {}
    h.fetch_done(id, Some(response(200, "https://a.test/k", &[], b"kept")));
    run(&mut e, &mut h);
    assert_eq!(eval(&mut e, &mut h, "return done;"), "kept");
    eval(&mut e, &mut h, "window.failed = 0; for (let i = 0; i < 70; i++) { const x = new XMLHttpRequest(); x.open('GET', '/n' + i); x.onerror = () => failed++; x.send(); }");
    assert_eq!(h.take_fetches().len(), 64);
    run(&mut e, &mut h);
    assert_eq!(eval(&mut e, &mut h, "return failed;"), "6");
    h.close(&mut e);
}
