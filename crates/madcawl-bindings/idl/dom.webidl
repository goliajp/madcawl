// the part of the DOM Standard's IDL bound so far, as written there
// except where noted; members not listed are not bound yet
//
// [HostOps] marks the class whose table carries the host object
// callbacks; its subclasses inherit them

[Exposed=Window]
interface EventTarget {
  // the options dictionary is not bound yet: only the capture boolean;
  // [Atom] (not in the standard) takes the type as an atom
  undefined addEventListener([Atom] DOMString type, EventListener? callback, optional boolean capture = false);
  undefined removeEventListener([Atom] DOMString type, EventListener? callback, optional boolean capture = false);
};

[Exposed=Window, HostOps]
interface Node : EventTarget {
  const unsigned short ELEMENT_NODE = 1;
  const unsigned short TEXT_NODE = 3;
  const unsigned short PROCESSING_INSTRUCTION_NODE = 7;
  const unsigned short COMMENT_NODE = 8;
  const unsigned short DOCUMENT_NODE = 9;
  const unsigned short DOCUMENT_TYPE_NODE = 10;
  const unsigned short DOCUMENT_FRAGMENT_NODE = 11;
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

  boolean contains(Node? other);

  [CEReactions] Node insertBefore(Node node, Node? child);
  [CEReactions] Node appendChild(Node node);
  [CEReactions] Node removeChild(Node child);
};

[Exposed=Window]
interface Element : Node {
  readonly attribute DOMString localName;
  readonly attribute DOMString tagName;

  // reflected attributes are readonly until attributes can be set
  readonly attribute DOMString id;
  readonly attribute DOMString className;

  DOMString? getAttribute(DOMString qualifiedName);
  boolean hasAttribute(DOMString qualifiedName);
};

[Exposed=Window]
interface Document : Node {
  readonly attribute Element? documentElement;

  // the text and comment nodes are bound as Node until their interfaces are
  [NewObject] Element createElement(DOMString localName);
  [NewObject] Node createTextNode(DOMString data);
  [NewObject] Node createComment(DOMString data);

  Element? getElementById(DOMString elementId);
};
