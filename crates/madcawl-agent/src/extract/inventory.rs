//! Every link, image and form control in scope, one record each.

use std::collections::HashMap;
use std::rc::Rc;

use madcawl_dom::{NodeId, NodeKind};
use madcawl_str::atom;

use super::{Records, Row, Value, What, attr, is_form, landmark, reference};
use crate::html_table::is_html;
use crate::record::{CHECKED, PASSWORD, REQUIRED};
use crate::role::Role;
use crate::states::MASK;
use crate::view::Reader;

fn text(k: &str, v: impl Into<String>) -> (Rc<str>, Value) {
    (Rc::from(k), Value::Text(v.into()))
}

fn in_scope<'a>(r: &'a Reader) -> impl Iterator<Item = NodeId> + 'a {
    let root = r.root();
    std::iter::once(root).chain(r.dom.descendants(root))
}

/// Links: their text, address and the landmark they are in.
pub(super) fn links(r: &Reader) -> Records {
    let mut recs = Records::new(What::Links);
    recs.columns = ["text", "url", "landmark"].map(String::from).to_vec();
    for n in in_scope(r) {
        let Some(rec) = r.writes(n).filter(|rec| rec.role == Role::Link) else {
            continue;
        };
        let fields = vec![
            text("text", r.name(&rec)),
            text("url", r.tree.value(&rec)),
            text("landmark", landmark(r, n)),
        ];
        recs.rows.push(Row {
            node: n,
            reference: r.reference(n, &rec).text(),
            fields,
        });
    }
    recs
}

/// Shown, or hidden ones asked for, by style alone: an image with an
/// empty `alt` has no role and so no record of its visibility, and an
/// image is not loaded, so one without `width` and `height` has no area
/// yet.
fn shown(r: &Reader, n: NodeId) -> bool {
    let styled = r.inputs.styles.is_none_or(|s| s.is_visible(r.dom, n));
    r.opts.include_hidden || (styled && !r.tree.transparent(r.inputs, r.dom, n))
}

/// Images: `img` elements and elements with the image role.
pub(super) fn images(r: &Reader) -> Records {
    let mut recs = Records::new(What::Images);
    recs.columns = ["alt", "src", "width", "height", "landmark"]
        .map(String::from)
        .to_vec();
    for n in in_scope(r) {
        let role = r.record(n).map_or(Role::None, |rec| rec.role);
        let img = is_html(r.dom, n, atom!("img"));
        if !(img || role == Role::Image) || !shown(r, n) {
            continue;
        }
        let alt = match r.record(n).filter(|_| role == Role::Image) {
            Some(rec) => r.name(&rec).to_owned(),
            None => attr(r.dom, n, atom!("alt")).unwrap_or_default(),
        };
        let fields = vec![
            text("alt", alt),
            text("src", attr(r.dom, n, atom!("src")).unwrap_or_default()),
            text("width", attr(r.dom, n, atom!("width")).unwrap_or_default()),
            text(
                "height",
                attr(r.dom, n, atom!("height")).unwrap_or_default(),
            ),
            text("landmark", landmark(r, n)),
        ];
        recs.rows.push(Row {
            node: n,
            reference: reference(r, n),
            fields,
        });
    }
    recs
}

/// The forms of the document by ID, the first of each.
fn forms_by_id(r: &Reader) -> HashMap<String, NodeId> {
    let mut out = HashMap::new();
    let doc = r.dom.document();
    for f in r.dom.descendants(doc).filter(|&f| is_form(r.dom, f)) {
        if let Some(id) = attr(r.dom, f, atom!("id")) {
            out.entry(id).or_insert(f);
        }
    }
    out
}

/// The form a control belongs to: its `form` attribute's, else the one
/// it is in.
fn owner(r: &Reader, by_id: &HashMap<String, NodeId>, n: NodeId) -> Option<NodeId> {
    if let Some(id) = attr(r.dom, n, atom!("form")) {
        return by_id.get(&id).copied();
    }
    let mut up = r.dom.parent(n);
    while let Some(p) = up {
        if is_form(r.dom, p) {
            return Some(p);
        }
        up = r.dom.parent(p);
    }
    None
}

fn options(r: &Reader, select: NodeId) -> Vec<String> {
    r.dom
        .descendants(select)
        .filter(|&o| is_html(r.dom, o, atom!("option")))
        .map(|o| crate::text::collapsed(&crate::text::text_content(r.dom, o)))
        .collect()
}

/// A control's type: an input's `type` (`text` when it has none or an
/// unknown one is not told apart), else the element's name.
fn control_type(r: &Reader, n: NodeId) -> Option<String> {
    let name = r.dom.local_name(n).filter(|_| r.dom.in_html_namespace(n))?;
    let ty = attr(r.dom, n, atom!("type")).map(|t| t.trim().to_ascii_lowercase());
    Some(match name {
        _ if name == atom!("input") => ty
            .filter(|t| !t.is_empty())
            .unwrap_or_else(|| "text".into()),
        _ if name == atom!("button") => ty.unwrap_or_else(|| "submit".into()),
        _ if name == atom!("select") => "select".into(),
        _ if name == atom!("textarea") => "textarea".into(),
        _ => return None,
    })
}

fn control(
    r: &Reader,
    by_id: &HashMap<String, NodeId>,
    n: NodeId,
    ty: &str,
) -> Vec<(Rc<str>, Value)> {
    let rec = r.record(n).unwrap_or_default();
    let form = owner(r, by_id, n);
    let form_name = form.map(|f| {
        attr(r.dom, f, atom!("name"))
            .or_else(|| attr(r.dom, f, atom!("id")))
            .or_else(|| attr(r.dom, f, atom!("action")))
            .unwrap_or_default()
    });
    let value = if rec.has(PASSWORD) || ty == "password" {
        MASK.to_owned()
    } else if matches!(
        ty,
        "checkbox" | "radio" | "submit" | "button" | "reset" | "image" | "hidden"
    ) || is_html(r.dom, n, atom!("button"))
    {
        attr(r.dom, n, atom!("value")).unwrap_or_default()
    } else {
        r.tree.value(&rec).to_owned()
    };
    let mut f = vec![
        text("form", form.map(|f| reference(r, f)).unwrap_or_default()),
        text("form_name", form_name.unwrap_or_default()),
        text("name", attr(r.dom, n, atom!("name")).unwrap_or_default()),
        text("type", ty),
        text("label", r.name(&rec)),
        ("required".into(), Value::Flag(rec.has(REQUIRED))),
        text("value", value),
    ];
    if matches!(ty, "checkbox" | "radio") {
        f.push(("checked".into(), Value::Flag(rec.has(CHECKED))));
    }
    if is_html(r.dom, n, atom!("select")) {
        f.push(("options".into(), Value::List(options(r, n))));
    }
    f
}

/// Form controls: the form, the control's name, type, label, whether it
/// is required and its current value. A password's value is the mask;
/// a hidden input is listed only when hidden content is asked for.
pub(super) fn forms(r: &Reader) -> Records {
    let mut recs = Records::new(What::Forms);
    recs.columns = [
        "form",
        "form_name",
        "name",
        "type",
        "label",
        "required",
        "value",
    ]
    .map(String::from)
    .to_vec();
    let by_id = forms_by_id(r);
    for n in in_scope(r) {
        if r.dom.kind(n) != NodeKind::Element {
            continue;
        }
        let Some(ty) = control_type(r, n) else {
            continue;
        };
        let visible = if ty == "hidden" {
            r.opts.include_hidden
        } else {
            shown(r, n)
        };
        if !visible {
            continue;
        }
        recs.rows.push(Row {
            node: n,
            reference: reference(r, n),
            fields: control(r, &by_id, n, &ty),
        });
    }
    recs
}
