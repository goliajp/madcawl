//! Name fixups for foreign content: SVG's mixed-case names, which the
//! tokenizer has lowercased, and namespaced attributes.

use madcawl_str::{Atom, STATIC_ATOMS, atom};

use super::sink::{Adjust, AttrNamespace, AttrRef};
use crate::token::Attribute;

/// For each static atom, the name it is changed to, by pairs of
/// lowercased and mixed-case spellings.
const fn case_table(pairs: &[(&str, &str)]) -> [Option<Atom>; STATIC_ATOMS] {
    let mut out = [None; STATIC_ATOMS];
    let mut i = 0;
    while i < pairs.len() {
        let (Some(from), Some(to)) = (
            Atom::lookup_static(pairs[i].0.as_bytes()),
            Atom::lookup_static(pairs[i].1.as_bytes()),
        ) else {
            panic!("a case fixup between names that are not static atoms");
        };
        out[from.index() as usize] = Some(to);
        i += 1;
    }
    out
}

fn fixup(table: &[Option<Atom>; STATIC_ATOMS], name: Atom) -> Option<Atom> {
    table.get(name.index() as usize).copied().flatten()
}

static SVG_TAGS: [Option<Atom>; STATIC_ATOMS] = case_table(&[
    ("altglyph", "altGlyph"),
    ("altglyphdef", "altGlyphDef"),
    ("altglyphitem", "altGlyphItem"),
    ("animatecolor", "animateColor"),
    ("animatemotion", "animateMotion"),
    ("animatetransform", "animateTransform"),
    ("clippath", "clipPath"),
    ("feblend", "feBlend"),
    ("fecolormatrix", "feColorMatrix"),
    ("fecomponenttransfer", "feComponentTransfer"),
    ("fecomposite", "feComposite"),
    ("feconvolvematrix", "feConvolveMatrix"),
    ("fediffuselighting", "feDiffuseLighting"),
    ("fedisplacementmap", "feDisplacementMap"),
    ("fedistantlight", "feDistantLight"),
    ("fedropshadow", "feDropShadow"),
    ("feflood", "feFlood"),
    ("fefunca", "feFuncA"),
    ("fefuncb", "feFuncB"),
    ("fefuncg", "feFuncG"),
    ("fefuncr", "feFuncR"),
    ("fegaussianblur", "feGaussianBlur"),
    ("feimage", "feImage"),
    ("femerge", "feMerge"),
    ("femergenode", "feMergeNode"),
    ("femorphology", "feMorphology"),
    ("feoffset", "feOffset"),
    ("fepointlight", "fePointLight"),
    ("fespecularlighting", "feSpecularLighting"),
    ("fespotlight", "feSpotLight"),
    ("fetile", "feTile"),
    ("feturbulence", "feTurbulence"),
    ("foreignobject", "foreignObject"),
    ("glyphref", "glyphRef"),
    ("lineargradient", "linearGradient"),
    ("radialgradient", "radialGradient"),
    ("textpath", "textPath"),
]);

static SVG_ATTRS: [Option<Atom>; STATIC_ATOMS] = case_table(&[
    ("attributename", "attributeName"),
    ("attributetype", "attributeType"),
    ("basefrequency", "baseFrequency"),
    ("baseprofile", "baseProfile"),
    ("calcmode", "calcMode"),
    ("clippathunits", "clipPathUnits"),
    ("diffuseconstant", "diffuseConstant"),
    ("edgemode", "edgeMode"),
    ("filterunits", "filterUnits"),
    ("glyphref", "glyphRef"),
    ("gradienttransform", "gradientTransform"),
    ("gradientunits", "gradientUnits"),
    ("kernelmatrix", "kernelMatrix"),
    ("kernelunitlength", "kernelUnitLength"),
    ("keypoints", "keyPoints"),
    ("keysplines", "keySplines"),
    ("keytimes", "keyTimes"),
    ("lengthadjust", "lengthAdjust"),
    ("limitingconeangle", "limitingConeAngle"),
    ("markerheight", "markerHeight"),
    ("markerunits", "markerUnits"),
    ("markerwidth", "markerWidth"),
    ("maskcontentunits", "maskContentUnits"),
    ("maskunits", "maskUnits"),
    ("numoctaves", "numOctaves"),
    ("pathlength", "pathLength"),
    ("patterncontentunits", "patternContentUnits"),
    ("patterntransform", "patternTransform"),
    ("patternunits", "patternUnits"),
    ("pointsatx", "pointsAtX"),
    ("pointsaty", "pointsAtY"),
    ("pointsatz", "pointsAtZ"),
    ("preservealpha", "preserveAlpha"),
    ("preserveaspectratio", "preserveAspectRatio"),
    ("primitiveunits", "primitiveUnits"),
    ("refx", "refX"),
    ("refy", "refY"),
    ("repeatcount", "repeatCount"),
    ("repeatdur", "repeatDur"),
    ("requiredextensions", "requiredExtensions"),
    ("requiredfeatures", "requiredFeatures"),
    ("specularconstant", "specularConstant"),
    ("specularexponent", "specularExponent"),
    ("spreadmethod", "spreadMethod"),
    ("startoffset", "startOffset"),
    ("stddeviation", "stdDeviation"),
    ("stitchtiles", "stitchTiles"),
    ("surfacescale", "surfaceScale"),
    ("systemlanguage", "systemLanguage"),
    ("tablevalues", "tableValues"),
    ("targetx", "targetX"),
    ("targety", "targetY"),
    ("textlength", "textLength"),
    ("viewbox", "viewBox"),
    ("viewtarget", "viewTarget"),
    ("xchannelselector", "xChannelSelector"),
    ("ychannelselector", "yChannelSelector"),
    ("zoomandpan", "zoomAndPan"),
]);

/// The SVG element name for a lowercased tag name, if it differs.
pub(crate) fn svg_tag(name: Atom) -> Option<Atom> {
    fixup(&SVG_TAGS, name)
}

/// Namespace, prefix and local name of a namespaced attribute.
type Foreign = (AttrNamespace, Option<Atom>, Atom);

static FOREIGN_ATTRS: [(Atom, Foreign); 11] = {
    use AttrNamespace::{XLink, Xml, Xmlns};
    [
        (
            atom!("xlink:actuate"),
            (XLink, Some(atom!("xlink")), atom!("actuate")),
        ),
        (
            atom!("xlink:arcrole"),
            (XLink, Some(atom!("xlink")), atom!("arcrole")),
        ),
        (
            atom!("xlink:href"),
            (XLink, Some(atom!("xlink")), atom!("href")),
        ),
        (
            atom!("xlink:role"),
            (XLink, Some(atom!("xlink")), atom!("role")),
        ),
        (
            atom!("xlink:show"),
            (XLink, Some(atom!("xlink")), atom!("show")),
        ),
        (
            atom!("xlink:title"),
            (XLink, Some(atom!("xlink")), atom!("title")),
        ),
        (
            atom!("xlink:type"),
            (XLink, Some(atom!("xlink")), atom!("type")),
        ),
        (atom!("xml:lang"), (Xml, Some(atom!("xml")), atom!("lang"))),
        (
            atom!("xml:space"),
            (Xml, Some(atom!("xml")), atom!("space")),
        ),
        (atom!("xmlns"), (Xmlns, None, atom!("xmlns"))),
        (
            atom!("xmlns:xlink"),
            (Xmlns, Some(atom!("xmlns")), atom!("xlink")),
        ),
    ]
};

pub(crate) fn adjust_attr(a: &Attribute, name: Atom, adjust: Adjust) -> AttrRef<'_> {
    let plain = |local: Atom| AttrRef {
        ns: AttrNamespace::None,
        prefix: None,
        local,
        value: &a.value,
    };
    match adjust {
        Adjust::Html => return plain(name),
        Adjust::Svg => {
            if let Some(local) = fixup(&SVG_ATTRS, name) {
                return plain(local);
            }
        }
        Adjust::MathMl => {
            if name == atom!("definitionurl") {
                return plain(atom!("definitionURL"));
            }
        }
    }
    match FOREIGN_ATTRS.iter().find(|(n, _)| *n == name) {
        Some(&(_, (ns, prefix, local))) => AttrRef {
            ns,
            prefix,
            local,
            value: &a.value,
        },
        None => plain(name),
    }
}
