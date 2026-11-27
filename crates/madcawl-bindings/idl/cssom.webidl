// CSSOM's declaration block of an element's style attribute. The
// attributes of the supported properties, one for each of their names,
// are put on the prototype when the window is installed. The indexed
// getter gives no property past the end, item the empty string.
// parentRule is null: no rule is bound
[Exposed=Window, Side]
interface CSSStyleDeclaration {
  [CEReactions] attribute DOMString cssText;
  readonly attribute unsigned long length;
  getter DOMString item(unsigned long index);
  DOMString getPropertyValue(DOMString property);
  DOMString getPropertyPriority(DOMString property);
  [CEReactions] undefined setProperty(DOMString property, [LegacyNullToEmptyString] DOMString value, optional [LegacyNullToEmptyString] DOMString priority = "");
  [CEReactions] DOMString removeProperty(DOMString property);
  readonly attribute any parentRule;
  [CEReactions] attribute [LegacyNullToEmptyString] DOMString cssFloat;
};
