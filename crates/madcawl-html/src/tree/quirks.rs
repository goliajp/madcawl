//! Which DOCTYPEs put a document in quirks or limited-quirks mode.

use super::QuirksMode;
use crate::token::Doctype;

const PUBLIC_IS: &[&[u8]] = &[
    b"-//W3O//DTD W3 HTML Strict 3.0//EN//",
    b"-/W3C/DTD HTML 4.0 Transitional/EN",
    b"HTML",
];

const SYSTEM_IS: &[&[u8]] = &[b"http://www.ibm.com/data/dtd/v11/ibmxhtml1-transitional.dtd"];

const PUBLIC_STARTS: &[&[u8]] = &[
    b"+//Silmaril//dtd html Pro v0r11 19970101//",
    b"-//AS//DTD HTML 3.0 asWedit + extensions//",
    b"-//AdvaSoft Ltd//DTD HTML 3.0 asWedit + extensions//",
    b"-//IETF//DTD HTML 2.0 Level 1//",
    b"-//IETF//DTD HTML 2.0 Level 2//",
    b"-//IETF//DTD HTML 2.0 Strict Level 1//",
    b"-//IETF//DTD HTML 2.0 Strict Level 2//",
    b"-//IETF//DTD HTML 2.0 Strict//",
    b"-//IETF//DTD HTML 2.0//",
    b"-//IETF//DTD HTML 2.1E//",
    b"-//IETF//DTD HTML 3.0//",
    b"-//IETF//DTD HTML 3.2 Final//",
    b"-//IETF//DTD HTML 3.2//",
    b"-//IETF//DTD HTML 3//",
    b"-//IETF//DTD HTML Level 0//",
    b"-//IETF//DTD HTML Level 1//",
    b"-//IETF//DTD HTML Level 2//",
    b"-//IETF//DTD HTML Level 3//",
    b"-//IETF//DTD HTML Strict Level 0//",
    b"-//IETF//DTD HTML Strict Level 1//",
    b"-//IETF//DTD HTML Strict Level 2//",
    b"-//IETF//DTD HTML Strict Level 3//",
    b"-//IETF//DTD HTML Strict//",
    b"-//IETF//DTD HTML//",
    b"-//Metrius//DTD Metrius Presentational//",
    b"-//Microsoft//DTD Internet Explorer 2.0 HTML Strict//",
    b"-//Microsoft//DTD Internet Explorer 2.0 HTML//",
    b"-//Microsoft//DTD Internet Explorer 2.0 Tables//",
    b"-//Microsoft//DTD Internet Explorer 3.0 HTML Strict//",
    b"-//Microsoft//DTD Internet Explorer 3.0 HTML//",
    b"-//Microsoft//DTD Internet Explorer 3.0 Tables//",
    b"-//Netscape Comm. Corp.//DTD HTML//",
    b"-//Netscape Comm. Corp.//DTD Strict HTML//",
    b"-//O'Reilly and Associates//DTD HTML 2.0//",
    b"-//O'Reilly and Associates//DTD HTML Extended 1.0//",
    b"-//O'Reilly and Associates//DTD HTML Extended Relaxed 1.0//",
    b"-//SQ//DTD HTML 2.0 HoTMetaL + extensions//",
    b"-//SoftQuad Software//DTD HoTMetaL PRO 6.0::19990601::extensions to HTML 4.0//",
    b"-//SoftQuad//DTD HoTMetaL PRO 4.0::19971010::extensions to HTML 4.0//",
    b"-//Spyglass//DTD HTML 2.0 Extended//",
    b"-//Sun Microsystems Corp.//DTD HotJava HTML//",
    b"-//Sun Microsystems Corp.//DTD HotJava Strict HTML//",
    b"-//W3C//DTD HTML 3 1995-03-24//",
    b"-//W3C//DTD HTML 3.2 Draft//",
    b"-//W3C//DTD HTML 3.2 Final//",
    b"-//W3C//DTD HTML 3.2//",
    b"-//W3C//DTD HTML 3.2S Draft//",
    b"-//W3C//DTD HTML 4.0 Frameset//",
    b"-//W3C//DTD HTML 4.0 Transitional//",
    b"-//W3C//DTD HTML Experimental 19960712//",
    b"-//W3C//DTD HTML Experimental 970421//",
    b"-//W3C//DTD W3 HTML//",
    b"-//W3O//DTD W3 HTML 3.0//",
    b"-//WebTechs//DTD Mozilla HTML 2.0//",
    b"-//WebTechs//DTD Mozilla HTML//",
];

const PUBLIC_STARTS_NO_SYSTEM: &[&[u8]] = &[
    b"-//W3C//DTD HTML 4.01 Frameset//",
    b"-//W3C//DTD HTML 4.01 Transitional//",
];

const LIMITED_PUBLIC_STARTS: &[&[u8]] = &[
    b"-//W3C//DTD XHTML 1.0 Frameset//",
    b"-//W3C//DTD XHTML 1.0 Transitional//",
];
fn starts_with_ci(s: &[u8], prefix: &[u8]) -> bool {
    s.len() >= prefix.len() && s[..prefix.len()].eq_ignore_ascii_case(prefix)
}

pub(crate) fn doctype_mode(d: &Doctype) -> QuirksMode {
    if d.force_quirks || d.name.as_deref() != Some(b"html") {
        return QuirksMode::Quirks;
    }
    let public = d.public_id.as_deref();
    let system = d.system_id.as_deref();
    let no_system = system.is_none_or(|s| s.is_empty());
    if let Some(p) = public
        && (PUBLIC_IS.iter().any(|q| p.eq_ignore_ascii_case(q))
            || PUBLIC_STARTS.iter().any(|q| starts_with_ci(p, q))
            || (no_system && PUBLIC_STARTS_NO_SYSTEM.iter().any(|q| starts_with_ci(p, q))))
    {
        return QuirksMode::Quirks;
    }
    if system.is_some_and(|s| SYSTEM_IS.iter().any(|q| s.eq_ignore_ascii_case(q))) {
        return QuirksMode::Quirks;
    }
    if let Some(p) = public
        && (LIMITED_PUBLIC_STARTS.iter().any(|q| starts_with_ci(p, q))
            || (!no_system && PUBLIC_STARTS_NO_SYSTEM.iter().any(|q| starts_with_ci(p, q))))
    {
        return QuirksMode::LimitedQuirks;
    }
    QuirksMode::NoQuirks
}
