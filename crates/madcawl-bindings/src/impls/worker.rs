//! The Worker interface's members (`workers.rs` has the worker).

use madcawl_js::{Engine, Root};
use madcawl_str::{DomString, atom};

use crate::convert::{Any, Thrown, throw_dom};
use crate::dict::{convert, dictionary};
use crate::event::Target;
use crate::fetches::{Credentials, FetchRequest, Mode, Owner};
use crate::frames::same_origin;
use crate::host::DomHost;
use crate::side::{SideData, SideRef};
use crate::workers::{MAX_PER_WINDOW, Worker, WorkerTask};

fn id_of<E: Engine>(h: &DomHost<E>, r: SideRef) -> u64 {
    match h.sides.get(r).data {
        SideData::Worker(w) => w,
        _ => unreachable!("a Worker's side object"),
    }
}

/// WorkerOptions: its `name`, after a `type` other than "classic" is refused.
fn options<E: Engine>(e: &mut E, h: &mut DomHost<E>, v: E::Value) -> Result<DomString, Thrown> {
    let Some(d) = dictionary(e, v, "the worker options")? else { return Ok(DomString::empty()) };
    let name = convert(e, h, d, "name", |e, h, v| e.to_string(h, v).ok_or(Thrown))?.unwrap_or_else(DomString::empty);
    match convert(e, h, d, "type", |e, h, v| e.to_string(h, v).ok_or(Thrown))? {
        Some(t) if t.eq_str("module") => Err(throw_dom(e, h, "NotSupportedError", "module workers are not supported yet")),
        Some(t) if !t.eq_str("classic") => Err(crate::convert::throw(e, madcawl_js::ErrorKind::Type, "not a worker type")),
        _ => Ok(name),
    }
}

/// Where a worker's script comes from: a blob URL's text, read now, or
/// a fetch of the window's origin.
enum Source {
    Text(DomString),
    Fetch(alt_url::Url),
}

pub(crate) fn constructor<E: Engine>(e: &mut E, h: &mut DomHost<E>, url: String, opts: E::Value) -> Result<SideRef, Thrown> {
    let realm = h.caller_realm(e)?;
    let owner = h.window_of_realm(realm).expect("a live window's realm");
    let name = options(e, h, opts)?;
    let doc = h.doc_of_window(owner).expect("a live window's document");
    let Ok(parsed) = h.base_url(doc).join(&url) else { return Err(throw_dom(e, h, "SyntaxError", "the script URL does not parse")) };
    let origin = h.origin_of(owner);
    if !same_origin(&parsed.origin(), &origin) {
        return Err(throw_dom(e, h, "SecurityError", "a worker script of another origin"));
    }
    let source = match parsed.scheme() {
        "blob" => h.blob_urls.get(parsed.as_str()).map(|b| Source::Text(DomString::from_wtf8(super::blob::utf8_decode(b.bytes()).as_bytes()))),
        "http" | "https" => Some(Source::Fetch(parsed.clone())),
        _ => None,
    };
    let object = h.sides.add(SideData::Worker(0));
    let wrapper = Root::new(h.wrap_side(e, object));
    let Some(source) = source.filter(|_| h.workers_of(owner) < MAX_PER_WINDOW) else {
        h.queue_worker(WorkerTask::Failed { owner, object, wrapper });
        return Ok(object);
    };
    let w = h.frames.take_id();
    h.sides.get_mut(object).data = SideData::Worker(w);
    let k = Worker { owner, object, wrapper, realm: e.new_realm(), url: parsed, origin, listeners: Vec::new(), started: false, inbox: Vec::new(), closing: false, request: None };
    h.workers.live.insert(w, k);
    h.install_worker_global(e, w, &name);
    match source {
        Source::Text(text) => h.queue_worker(WorkerTask::Run(w, text)),
        Source::Fetch(u) => {
            let (url, initiator) = (u.as_str().to_owned(), h.initiator_of(owner));
            let (credentials, mode) = (Credentials::SameOrigin, Mode::SameOrigin);
            let req = FetchRequest { id: 0, destination: "worker", method: "GET".to_owned(), url, headers: Vec::new(), body: None, initiator, credentials, mode };
            let id = h.start_fetch(req, Owner::Worker(w), None);
            h.workers.live.get_mut(&w).expect("just made").request = Some(id);
        }
    }
    Ok(object)
}

pub(crate) fn terminate<E: Engine>(e: &mut E, h: &mut DomHost<E>, this: SideRef) -> Result<(), Thrown> {
    let w = id_of(h, this);
    h.destroy_worker(e, w);
    Ok(())
}

pub(crate) fn post_message<E: Engine>(e: &mut E, h: &mut DomHost<E>, this: SideRef, message: E::Value, _transfer: E::Value) -> Result<(), Thrown> {
    let w = id_of(h, this);
    h.post_to_worker(e, w, message)
}

macro_rules! handlers {
    ($($get:ident, $set:ident => $ty:literal;)*) => {$(
        pub(crate) fn $get<E: Engine>(e: &mut E, h: &mut DomHost<E>, this: SideRef) -> Result<Any<E>, Thrown> {
            Ok(Any(h.event_handler(e, Target::Side(this), atom!($ty))))
        }

        pub(crate) fn $set<E: Engine>(e: &mut E, h: &mut DomHost<E>, this: SideRef, value: Option<E::Value>) -> Result<(), Thrown> {
            h.set_event_handler(e, Target::Side(this), atom!($ty), value);
            Ok(())
        }
    )*};
}

handlers! {
    onmessage, set_onmessage => "message";
    onmessageerror, set_onmessageerror => "messageerror";
    onerror, set_onerror => "error";
}
