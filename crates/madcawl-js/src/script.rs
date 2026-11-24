//! Where a script came from, and a module the engine asks for.

use madcawl_str::{Atom, DomString};

pub struct ScriptOrigin<'a> {
    pub url: &'a DomString,
    pub line: u32,
    pub column: u32,
}

pub struct ModuleRequest<'a> {
    pub referrer: Option<&'a DomString>,
    pub specifier: &'a DomString,
    /// The `type` import attribute.
    pub module_type: Option<Atom>,
}
