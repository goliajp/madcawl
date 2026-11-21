//! Name fixups for foreign content: SVG's mixed-case names, which the
//! tokenizer has lowercased, and namespaced attributes.

use super::sink::{Adjust, AttrNamespace, AttrRef};
use crate::token::Attribute;

/// The SVG element name for a lowercased tag name, if it differs.
pub(crate) fn svg_tag(name: &[u8]) -> Option<&'static [u8]> {
    Some(match name {
        b"altglyph" => b"altGlyph",
        b"altglyphdef" => b"altGlyphDef",
        b"altglyphitem" => b"altGlyphItem",
        b"animatecolor" => b"animateColor",
        b"animatemotion" => b"animateMotion",
        b"animatetransform" => b"animateTransform",
        b"clippath" => b"clipPath",
        b"feblend" => b"feBlend",
        b"fecolormatrix" => b"feColorMatrix",
        b"fecomponenttransfer" => b"feComponentTransfer",
        b"fecomposite" => b"feComposite",
        b"feconvolvematrix" => b"feConvolveMatrix",
        b"fediffuselighting" => b"feDiffuseLighting",
        b"fedisplacementmap" => b"feDisplacementMap",
        b"fedistantlight" => b"feDistantLight",
        b"fedropshadow" => b"feDropShadow",
        b"feflood" => b"feFlood",
        b"fefunca" => b"feFuncA",
        b"fefuncb" => b"feFuncB",
        b"fefuncg" => b"feFuncG",
        b"fefuncr" => b"feFuncR",
        b"fegaussianblur" => b"feGaussianBlur",
        b"feimage" => b"feImage",
        b"femerge" => b"feMerge",
        b"femergenode" => b"feMergeNode",
        b"femorphology" => b"feMorphology",
        b"feoffset" => b"feOffset",
        b"fepointlight" => b"fePointLight",
        b"fespecularlighting" => b"feSpecularLighting",
        b"fespotlight" => b"feSpotLight",
        b"fetile" => b"feTile",
        b"feturbulence" => b"feTurbulence",
        b"foreignobject" => b"foreignObject",
        b"glyphref" => b"glyphRef",
        b"lineargradient" => b"linearGradient",
        b"radialgradient" => b"radialGradient",
        b"textpath" => b"textPath",
        _ => return None,
    })
}

fn svg_attr(name: &[u8]) -> Option<&'static [u8]> {
    Some(match name {
        b"attributename" => b"attributeName",
        b"attributetype" => b"attributeType",
        b"basefrequency" => b"baseFrequency",
        b"baseprofile" => b"baseProfile",
        b"calcmode" => b"calcMode",
        b"clippathunits" => b"clipPathUnits",
        b"diffuseconstant" => b"diffuseConstant",
        b"edgemode" => b"edgeMode",
        b"filterunits" => b"filterUnits",
        b"glyphref" => b"glyphRef",
        b"gradienttransform" => b"gradientTransform",
        b"gradientunits" => b"gradientUnits",
        b"kernelmatrix" => b"kernelMatrix",
        b"kernelunitlength" => b"kernelUnitLength",
        b"keypoints" => b"keyPoints",
        b"keysplines" => b"keySplines",
        b"keytimes" => b"keyTimes",
        b"lengthadjust" => b"lengthAdjust",
        b"limitingconeangle" => b"limitingConeAngle",
        b"markerheight" => b"markerHeight",
        b"markerunits" => b"markerUnits",
        b"markerwidth" => b"markerWidth",
        b"maskcontentunits" => b"maskContentUnits",
        b"maskunits" => b"maskUnits",
        b"numoctaves" => b"numOctaves",
        b"pathlength" => b"pathLength",
        b"patterncontentunits" => b"patternContentUnits",
        b"patterntransform" => b"patternTransform",
        b"patternunits" => b"patternUnits",
        b"pointsatx" => b"pointsAtX",
        b"pointsaty" => b"pointsAtY",
        b"pointsatz" => b"pointsAtZ",
        b"preservealpha" => b"preserveAlpha",
        b"preserveaspectratio" => b"preserveAspectRatio",
        b"primitiveunits" => b"primitiveUnits",
        b"refx" => b"refX",
        b"refy" => b"refY",
        b"repeatcount" => b"repeatCount",
        b"repeatdur" => b"repeatDur",
        b"requiredextensions" => b"requiredExtensions",
        b"requiredfeatures" => b"requiredFeatures",
        b"specularconstant" => b"specularConstant",
        b"specularexponent" => b"specularExponent",
        b"spreadmethod" => b"spreadMethod",
        b"startoffset" => b"startOffset",
        b"stddeviation" => b"stdDeviation",
        b"stitchtiles" => b"stitchTiles",
        b"surfacescale" => b"surfaceScale",
        b"systemlanguage" => b"systemLanguage",
        b"tablevalues" => b"tableValues",
        b"targetx" => b"targetX",
        b"targety" => b"targetY",
        b"textlength" => b"textLength",
        b"viewbox" => b"viewBox",
        b"viewtarget" => b"viewTarget",
        b"xchannelselector" => b"xChannelSelector",
        b"ychannelselector" => b"yChannelSelector",
        b"zoomandpan" => b"zoomAndPan",
        _ => return None,
    })
}

/// Namespace, prefix and local name of a namespaced attribute.
type Foreign = (AttrNamespace, Option<&'static [u8]>, &'static [u8]);

fn foreign_attr(name: &[u8]) -> Option<Foreign> {
    use AttrNamespace::{XLink, Xml, Xmlns};
    Some(match name {
        b"xlink:actuate" => (XLink, Some(b"xlink"), b"actuate"),
        b"xlink:arcrole" => (XLink, Some(b"xlink"), b"arcrole"),
        b"xlink:href" => (XLink, Some(b"xlink"), b"href"),
        b"xlink:role" => (XLink, Some(b"xlink"), b"role"),
        b"xlink:show" => (XLink, Some(b"xlink"), b"show"),
        b"xlink:title" => (XLink, Some(b"xlink"), b"title"),
        b"xlink:type" => (XLink, Some(b"xlink"), b"type"),
        b"xml:lang" => (Xml, Some(b"xml"), b"lang"),
        b"xml:space" => (Xml, Some(b"xml"), b"space"),
        b"xmlns" => (Xmlns, None, b"xmlns"),
        b"xmlns:xlink" => (Xmlns, Some(b"xmlns"), b"xlink"),
        _ => return None,
    })
}

pub(crate) fn adjust_attr(a: &Attribute, adjust: Adjust) -> AttrRef<'_> {
    let plain = |local: &'static [u8]| AttrRef {
        ns: AttrNamespace::None,
        prefix: None,
        local,
        value: &a.value,
    };
    match adjust {
        Adjust::Html => {}
        Adjust::Svg => {
            if let Some(local) = svg_attr(&a.name) {
                return plain(local);
            }
        }
        Adjust::MathMl => {
            if a.name == b"definitionurl" {
                return plain(b"definitionURL");
            }
        }
    }
    if adjust != Adjust::Html
        && let Some((ns, prefix, local)) = foreign_attr(&a.name)
    {
        return AttrRef {
            ns,
            prefix,
            local,
            value: &a.value,
        };
    }
    AttrRef {
        ns: AttrNamespace::None,
        prefix: None,
        local: &a.name,
        value: &a.value,
    }
}
