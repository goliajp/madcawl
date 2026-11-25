//! The anonymous table objects (CSS 2 17.2.1): a row's children that are not cells wrapped in an anonymous
//! cell, a table's or row group's children that are not what it holds wrapped in an anonymous row, and runs
//! of table-internal boxes outside a table wrapped in an anonymous table. Anonymous boxes are `AnonBlock`s
//! of their parent's element; their table role is kept beside the box tree.

use madcawl_css::style::{Display, Styles};

use crate::tree::{BoxNode, BoxTree, Kind, NONE};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Group {
    Head,
    Body,
    Foot,
}

/// What a box is to table layout.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Role {
    Table,
    Group(Group),
    Row,
    Cell,
    Column,
    ColumnGroup,
    Caption,
    Other,
}

impl Role {
    fn of_display(d: Display) -> Role {
        use Display::*;
        match d {
            Table | InlineTable => Role::Table,
            TableRowGroup => Role::Group(Group::Body),
            TableHeaderGroup => Role::Group(Group::Head),
            TableFooterGroup => Role::Group(Group::Foot),
            TableRow => Role::Row,
            TableCell => Role::Cell,
            TableColumn => Role::Column,
            TableColumnGroup => Role::ColumnGroup,
            TableCaption => Role::Caption,
            _ => Role::Other,
        }
    }

    fn internal(self) -> bool {
        matches!(
            self,
            Role::Group(_) | Role::Row | Role::Cell | Role::Column | Role::ColumnGroup
        )
    }

    // what a table holds directly
    fn proper(self) -> bool {
        matches!(
            self,
            Role::Group(_) | Role::Row | Role::Column | Role::ColumnGroup | Role::Caption
        )
    }
}

/// The roles of the anonymous table boxes, by box, in box order.
pub type Roles = Vec<(u32, Role)>;

/// The role of box `b`, whose element's display `display` gives.
pub fn role(t: &BoxTree, b: u32, display: impl Fn(u32) -> Option<Display>) -> Role {
    match t.get(b).kind {
        Kind::AnonBlock => t
            .table_roles
            .binary_search_by_key(&b, |&(x, _)| x)
            .map_or(Role::Other, |k| t.table_roles[k].1),
        Kind::Block | Kind::InlineBlock => display(b).map_or(Role::Other, Role::of_display),
        _ => Role::Other,
    }
}

fn relink(t: &mut BoxTree, p: u32, kids: &[u32]) {
    let (mut first, mut prev) = (NONE, NONE);
    for &k in kids {
        t.boxes[k as usize].parent = p;
        t.boxes[k as usize].next = NONE;
        if prev == NONE {
            first = k;
        } else {
            t.boxes[prev as usize].next = k;
        }
        prev = k;
    }
    t.boxes[p as usize].first = first;
    t.boxes[p as usize].last = prev;
}

fn wrap(t: &mut BoxTree, p: u32, run: &[u32], r: Role) -> u32 {
    let b = t.boxes.len() as u32;
    t.boxes.push(BoxNode {
        kind: Kind::AnonBlock,
        node: t.get(p).node,
        parent: p,
        first: NONE,
        last: NONE,
        next: NONE,
        pseudo: t.get(p).pseudo,
    });
    // the run came from a parent whose children were all inline-level or all block-level
    let inline = t.inline_children[p as usize];
    t.inline_children.push(inline);
    t.table_roles.push((b, r));
    relink(t, b, run);
    b
}

// the children of `p` with each run of those `stray` says do not belong wrapped in a box of role `r`
fn fix_children(t: &mut BoxTree, p: u32, roles: &[Role], stray: impl Fn(Role) -> bool, r: Role) {
    let kids: Vec<u32> = t.children(p).collect();
    if !roles.iter().any(|&x| stray(x)) {
        return;
    }
    let mut out = Vec::with_capacity(kids.len());
    let mut k = 0;
    while k < kids.len() {
        if !stray(roles[k]) {
            out.push(kids[k]);
            k += 1;
            continue;
        }
        let end = k + roles[k..].iter().take_while(|&&x| stray(x)).count();
        let w = wrap(t, p, &kids[k..end], r);
        out.push(w);
        k = end;
    }
    relink(t, p, &out);
    t.inline_children[p as usize] = false;
}

/// Tables nested deeper than this get no anonymous tables around their stray internal boxes, which lay out
/// as blocks: it bounds the recursion of table layout as `MAX_DEPTH` bounds that of the elements, whose
/// tables take four elements a level.
pub const MAX_TABLE_DEPTH: usize = 64;

/// Adds the anonymous table boxes the tree needs; nothing when it has no table-internal box.
/// The display of box `b`'s own computed values: a generated box's are its pseudo-element's.
pub fn display(t: &BoxTree, styles: &Styles, b: u32) -> Option<Display> {
    let bx = t.get(b);
    match bx.pseudo {
        0 => styles.get(bx.node),
        k => styles.pseudo(bx.node, k == 2),
    }
    .map(|s| s.display)
}

pub fn fix(t: &mut BoxTree, styles: &Styles) {
    let display = |t: &BoxTree, b: u32| display(t, styles, b);
    let any = (0..t.len() as u32).any(|b| role(t, b, |x| display(t, x)) != Role::Other);
    if !any {
        return;
    }
    let mut stack = vec![(0u32, 0usize)];
    while let Some((p, depth)) = stack.pop() {
        let pr = role(t, p, |x| display(t, x));
        let roles: Vec<Role> = t
            .children(p)
            .map(|c| role(t, c, |x| display(t, x)))
            .collect();
        match pr {
            Role::Row => fix_children(t, p, &roles, |x| x != Role::Cell, Role::Cell),
            Role::Table => fix_children(t, p, &roles, |x| !x.proper(), Role::Row),
            Role::Group(_) => fix_children(t, p, &roles, |x| x != Role::Row, Role::Row),
            Role::Column | Role::ColumnGroup => {}
            Role::Cell | Role::Caption | Role::Other if depth < MAX_TABLE_DEPTH => {
                fix_children(t, p, &roles, Role::internal, Role::Table)
            }
            Role::Cell | Role::Caption | Role::Other => {}
        }
        let depth = depth + usize::from(pr == Role::Table);
        stack.extend(t.children(p).map(|c| (c, depth)));
    }
}
