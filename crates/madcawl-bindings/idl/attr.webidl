// Attr and NamedNodeMap of the DOM Standard, as written there except
// where noted; members not listed are not bound yet

// [Side]: an element's attributes are not nodes in this DOM, and the map
// is a side object of the element
[Exposed=Window, Side]
interface NamedNodeMap {
  readonly attribute unsigned long length;
  getter Attr? item(unsigned long index);
  getter Attr? getNamedItem(DOMString qualifiedName);
  Attr? getNamedItemNS(DOMString? namespace, DOMString localName);
  [CEReactions, RunsScript] Attr? setNamedItem(Attr attr);
  [CEReactions, RunsScript] Attr? setNamedItemNS(Attr attr);
  [CEReactions] Attr removeNamedItem(DOMString qualifiedName);
  [CEReactions] Attr removeNamedItemNS(DOMString? namespace, DOMString localName);
};

// [Side]: an Attr is a side object, not a node of the DOM. Node's own
// members take nodes only and throw a TypeError for an Attr; the ones
// below them (not in the standard) are its own, as Node gives them for
// an attribute
[Exposed=Window, Side]
interface Attr : Node {
  readonly attribute DOMString? namespaceURI;
  readonly attribute DOMString? prefix;
  readonly attribute DOMString localName;
  readonly attribute DOMString name;
  [CEReactions] attribute DOMString value;

  readonly attribute Element? ownerElement;

  readonly attribute boolean specified;

  readonly attribute unsigned short nodeType;
  readonly attribute DOMString nodeName;
  readonly attribute boolean isConnected;
  readonly attribute Document? ownerDocument;
  readonly attribute Node? parentNode;
  readonly attribute Element? parentElement;
  boolean hasChildNodes();
  readonly attribute Node? firstChild;
  readonly attribute Node? lastChild;
  readonly attribute Node? previousSibling;
  readonly attribute Node? nextSibling;
  [CEReactions] attribute DOMString? textContent;
};
