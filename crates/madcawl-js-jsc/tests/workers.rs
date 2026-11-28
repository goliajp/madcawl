//! Dedicated workers on JavaScriptCore: a worker from a blob URL or
//! fetched from the window's origin, messages cloned both ways, errors,
//! terminate and close, and the bound on workers per window, freed with
//! the window.

use madcawl_bindings::{Credentials, DomHost, FetchResponse, Mode};
use madcawl_dom::Dom;
use madcawl_js::{Engine, ScriptOrigin};
use madcawl_js_jsc::Jsc;
use madcawl_str::DomString;

fn page() -> (Jsc, DomHost<Jsc>) {
    let mut e = Jsc::new();
    let mut h = DomHost::new(&mut e, Dom::new());
    assert!(h.set_url("https://a.test/dir/page"));
    h.install_window(&mut e);
    h.use_virtual_time();
    h.parse_document(&mut e, b"<body>");
    run(&mut e, &mut h);
    (e, h)
}

fn run(e: &mut Jsc, h: &mut DomHost<Jsc>) {
    while h.run_tasks(e, 100) != 0 {}
}

/// The result of `src` as a string, or what it threw.
fn eval(e: &mut Jsc, h: &mut DomHost<Jsc>, src: &str) -> String {
    let url = DomString::from_wtf8(b"workers");
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

/// `window.spawn(src)`: a worker running `src` from a blob URL, its
/// messages logged in `log`.
const SPAWN: &str = "window.log = []; window.spawn = function (src, opts) {
  const u = URL.createObjectURL(new Blob([src], { type: 'text/javascript' }));
  const w = new Worker(u, opts); URL.revokeObjectURL(u);
  w.onmessage = e => log.push(typeof e.data === 'string' ? e.data : JSON.stringify(e.data));
  return w;
};";

#[test]
fn the_challenge_s_probe_makes_a_worker_from_a_blob_url_and_terminates_it() {
    let (mut e, mut h) = page();
    let src = "var u = URL.createObjectURL(new Blob(['\"you\"===\"bot\"'], { type: 'text/javascript' })); var w = new Worker(u);
URL.revokeObjectURL(u); w.terminate(); w.terminate(); return [w instanceof Worker, w instanceof EventTarget, Object.prototype.toString.call(w)].join();";
    assert_eq!(eval(&mut e, &mut h, src), "true,true,[object Worker]");
    assert_eq!(h.worker_count(), 0);
    run(&mut e, &mut h);
    assert!(h.take_notes().is_empty());
    h.close(&mut e);
}

#[test]
fn messages_are_cloned_into_the_other_realm_and_wait_for_the_script() {
    let (mut e, mut h) = page();
    eval(&mut e, &mut h, SPAWN);
    let worker = "onmessage = e => { const d = e.data; postMessage({ map: d.m instanceof Map, mv: d.m.get(1), date: d.d instanceof Date && d.d.getTime(),
  bytes: d.b instanceof Uint8Array && Array.from(d.b).join(' '), buf: d.buf instanceof ArrayBuffer && d.buf.byteLength, cyc: d.self === d,
  arr: Array.isArray(d.a) && d.a.length, re: d.r instanceof RegExp && d.r.flags, err: d.err instanceof TypeError && d.err.message,
  ev: e instanceof MessageEvent, origin: e.origin, name: self.name, self: self === globalThis }); };
postMessage('ready');";
    let src = "window.w = spawn(WORKER, { name: 'n1' }); const o = { m: new Map([[1, 'x']]), d: new Date(5), b: new Uint8Array([1, 2, 3]).subarray(1),
  buf: new ArrayBuffer(4), a: [1, , 3], r: /a/gi, err: new TypeError('t') }; o.self = o; w.postMessage(o); o.m.set(1, 'changed'); return log.join();";
    assert_eq!(eval(&mut e, &mut h, &src.replace("WORKER", &format!("{worker:?}"))), "");
    run(&mut e, &mut h);
    let want = "ready|{\"map\":true,\"mv\":\"x\",\"date\":5,\"bytes\":\"2 3\",\"buf\":4,\"cyc\":true,\"arr\":3,\"re\":\"gi\",\"err\":\"t\",\"ev\":true,\"origin\":\"\",\"name\":\"n1\",\"self\":true}";
    assert_eq!(eval(&mut e, &mut h, "return log.join('|');"), want);
    let src = "function name(f) { try { f(); return 'none'; } catch (x) { return x.name; } }
return [name(() => w.postMessage(() => 1)), name(() => w.postMessage({ s: Symbol() })), name(() => w.postMessage({ get x() { throw new RangeError('g'); } })),
  name(() => w.postMessage())].join();";
    assert_eq!(eval(&mut e, &mut h, src), "DataCloneError,DataCloneError,RangeError,TypeError");
    h.close(&mut e);
}

#[test]
fn a_worker_has_timers_listeners_and_closes_itself() {
    let (mut e, mut h) = page();
    eval(&mut e, &mut h, SPAWN);
    let worker = "addEventListener('message', e => { setTimeout(() => { postMessage('timer ' + e.data); postMessage('closing'); close(); postMessage('after close'); }, 10); });
setInterval(() => {}, 1000);";
    eval(&mut e, &mut h, &format!("window.w = spawn({worker:?}); w.postMessage('a');"));
    run(&mut e, &mut h);
    assert_eq!(eval(&mut e, &mut h, "w.postMessage('dropped'); return log.join('|');"), "timer a|closing|after close");
    assert_eq!(h.worker_count(), 0);
    assert_eq!(h.event_loop().pending(), 0, "the closed worker's interval is gone");
    h.close(&mut e);
}

#[test]
fn an_uncaught_error_goes_to_the_global_then_the_worker_object() {
    let (mut e, mut h) = page();
    eval(&mut e, &mut h, SPAWN);
    let src = "window.errs = [];
const a = spawn('throw new Error(\"boom\")'); a.onerror = ev => { errs.push([ev instanceof ErrorEvent, ev.message, /^blob:/.test(ev.filename), ev.lineno, ev.error, ev.cancelable].join()); ev.preventDefault(); };
const b = spawn('self.onerror = ev => { postMessage(\"inner \" + ev.message + \" \" + (ev.error instanceof Error)); ev.preventDefault(); }; onmessage = () => { throw new TypeError(\"t\"); };');
b.onerror = () => errs.push('b reached the object'); b.postMessage(1);
const c = spawn('null.x'); window.c = c; return '';";
    eval(&mut e, &mut h, src);
    run(&mut e, &mut h);
    assert_eq!(eval(&mut e, &mut h, "return errs.join('|') + '#' + log.join('|');"), "true,Uncaught Error: boom,true,1,,true#inner Uncaught TypeError: t true");
    let notes: Vec<String> = h.take_notes().iter().map(ToString::to_string).collect();
    assert_eq!(notes.len(), 1, "{notes:?}");
    assert!(notes[0].contains("in a worker: Uncaught TypeError") && notes[0].contains("blob:https://a.test/"), "{notes:?}");
    h.close(&mut e);
}

#[test]
fn a_script_url_is_checked_and_one_of_the_origin_is_fetched() {
    let (mut e, mut h) = page();
    eval(&mut e, &mut h, SPAWN);
    let src = "function name(f) { try { f(); return 'none'; } catch (x) { return x.name; } }
window.errs = 0; const r = [name(() => new Worker('https://b.test/w.js')), name(() => new Worker('http://[::1')), name(() => new Worker('w.js', { type: 'module' })),
  name(() => new Worker('w.js', { type: 'x' })), name(() => Worker('w.js'))];
window.w = new Worker('w.js'); w.onmessage = e => log.push('fetched ' + e.data); w.postMessage('early');
window.bad = new Worker('missing.js'); bad.onerror = ev => log.push('missing ' + ev.constructor.name);
window.gone = new Worker('blob:https://a.test/0000'); gone.onerror = () => log.push('revoked');
window.away = new Worker('away.js'); away.onerror = () => log.push('redirected');
return r.join();";
    assert_eq!(eval(&mut e, &mut h, src), "SecurityError,SyntaxError,NotSupportedError,TypeError,TypeError");
    let reqs = h.take_fetches();
    let got: Vec<(&str, &str, &str)> = reqs.iter().map(|r| (r.destination, r.method.as_str(), r.url.as_str())).collect();
    let want = [("worker", "GET", "https://a.test/dir/w.js"), ("worker", "GET", "https://a.test/dir/missing.js"), ("worker", "GET", "https://a.test/dir/away.js")];
    assert_eq!(got, want);
    for r in &reqs {
        assert_eq!((r.initiator.as_deref(), r.credentials, r.mode), (Some("https://a.test/dir/page"), Credentials::SameOrigin, Mode::SameOrigin));
    }
    let ok = FetchResponse { status: 200, status_text: "OK".into(), url: reqs[0].url.clone(), headers: Vec::new(), body: b"onmessage = e => postMessage(e.data)".to_vec() };
    h.fetch_done(reqs[0].id, Some(ok.clone()));
    h.fetch_done(reqs[1].id, Some(FetchResponse { status: 404, ..ok.clone() }));
    h.fetch_done(reqs[2].id, Some(FetchResponse { url: "https://b.test/away.js".into(), ..ok }));
    run(&mut e, &mut h);
    assert_eq!(eval(&mut e, &mut h, "return log.join('|');"), "revoked|missing Event|redirected|fetched early");
    h.close(&mut e);
}

#[test]
fn workers_are_bounded_per_window_and_go_with_it() {
    let (mut e, mut h) = page();
    eval(&mut e, &mut h, SPAWN);
    let src = "window.ws = []; window.failed = 0; for (let i = 0; i < 17; i++) { const w = spawn('postMessage(1)'); w.onerror = () => failed++; ws.push(w); }
const f = document.createElement('iframe'); document.body.appendChild(f); window.f = f;
const inner = f.contentWindow; const u = URL.createObjectURL(new Blob(['onmessage = () => {}']));
window.fw = new inner.Worker(u); return '';";
    eval(&mut e, &mut h, src);
    assert_eq!(h.worker_count(), 17, "16 for the window and one for its iframe");
    run(&mut e, &mut h);
    assert_eq!(eval(&mut e, &mut h, "return failed;"), "1");
    eval(&mut e, &mut h, "f.remove();");
    assert_eq!(h.worker_count(), 16, "the iframe's worker went with it");
    eval(&mut e, &mut h, "ws.forEach(w => w.terminate());");
    assert_eq!(h.worker_count(), 0);
    eval(&mut e, &mut h, "spawn('setInterval(() => postMessage(1), 1000)');");
    assert_eq!(h.worker_count(), 1);
    h.close(&mut e);
}

#[test]
fn platform_objects_are_not_cloned_and_an_error_for_a_gone_window_is_dropped() {
    let (mut e, mut h) = page();
    eval(&mut e, &mut h, SPAWN);
    let src = "function name(f) { try { f(); return 'none'; } catch (x) { return x.name; } }
const w = spawn('onmessage = e => postMessage(e.data)');
const r = [name(() => w.postMessage(new Blob(['b']))), name(() => w.postMessage({ el: document.body })), name(() => w.postMessage([new URLSearchParams('a=1')])),
  name(() => w.postMessage({ plain: [1, { two: 2 }] }))];
const f = document.createElement('iframe'); document.body.appendChild(f);
window.fw = new f.contentWindow.Worker('blob:https://a.test/none'); fw.onerror = () => log.push('error for a gone window');
f.remove(); return r.join();";
    assert_eq!(eval(&mut e, &mut h, src), "DataCloneError,DataCloneError,DataCloneError,none");
    run(&mut e, &mut h);
    assert_eq!(eval(&mut e, &mut h, "return log.join('|');"), "{\"plain\":[1,{\"two\":2}]}");
    h.close(&mut e);
}
