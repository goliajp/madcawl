// the part of the DOM Standard's IDL bound so far, as written there
// except where noted; members not listed are not bound yet
//
// [HostOps] marks the class whose table carries the host object
// callbacks; its subclasses inherit them

// [Targets] (not in the standard): its own objects are side objects,
// and its members take nodes and those as `this`
[Exposed=*, Targets]
interface EventTarget {
  constructor();

  // the options, a boolean or a dictionary, are read as any: of the
  // dictionary capture, once and passive are acted on, and a signal
  // that is present throws a TypeError, since no AbortSignal is bound
  // yet; [Atom] (not in the standard) takes the type as an atom
  undefined addEventListener([Atom] DOMString type, EventListener? callback, optional any options);
  undefined removeEventListener([Atom] DOMString type, EventListener? callback, optional any options);
  boolean dispatchEvent(Event event);
};

// from Web IDL; [ErrorPrototype] (not in the standard): its interface
// prototype object inherits from Error.prototype, as Web IDL says of it
// alone
[Exposed=*, Side, ErrorPrototype]
interface DOMException {
  constructor(optional DOMString message = "", optional DOMString name = "Error");
  readonly attribute DOMString name;
  readonly attribute DOMString message;
  readonly attribute unsigned short code;

  const unsigned short INDEX_SIZE_ERR = 1;
  const unsigned short DOMSTRING_SIZE_ERR = 2;
  const unsigned short HIERARCHY_REQUEST_ERR = 3;
  const unsigned short WRONG_DOCUMENT_ERR = 4;
  const unsigned short INVALID_CHARACTER_ERR = 5;
  const unsigned short NO_DATA_ALLOWED_ERR = 6;
  const unsigned short NO_MODIFICATION_ALLOWED_ERR = 7;
  const unsigned short NOT_FOUND_ERR = 8;
  const unsigned short NOT_SUPPORTED_ERR = 9;
  const unsigned short INUSE_ATTRIBUTE_ERR = 10;
  const unsigned short INVALID_STATE_ERR = 11;
  const unsigned short SYNTAX_ERR = 12;
  const unsigned short INVALID_MODIFICATION_ERR = 13;
  const unsigned short NAMESPACE_ERR = 14;
  const unsigned short INVALID_ACCESS_ERR = 15;
  const unsigned short VALIDATION_ERR = 16;
  const unsigned short TYPE_MISMATCH_ERR = 17;
  const unsigned short SECURITY_ERR = 18;
  const unsigned short NETWORK_ERR = 19;
  const unsigned short ABORT_ERR = 20;
  const unsigned short URL_MISMATCH_ERR = 21;
  const unsigned short QUOTA_EXCEEDED_ERR = 22;
  const unsigned short TIMEOUT_ERR = 23;
  const unsigned short INVALID_NODE_TYPE_ERR = 24;
  const unsigned short DATA_CLONE_ERR = 25;
};

// [Side] (not in the standard): its objects are host objects that are
// not nodes. The EventInit dictionary is read as any
[Exposed=*, Side]
interface Event {
  constructor(DOMString type, optional any eventInitDict);

  readonly attribute DOMString type;
  readonly attribute EventTarget? target;
  readonly attribute EventTarget? srcElement;
  readonly attribute EventTarget? currentTarget;
  sequence<EventTarget> composedPath();

  const unsigned short NONE = 0;
  const unsigned short CAPTURING_PHASE = 1;
  const unsigned short AT_TARGET = 2;
  const unsigned short BUBBLING_PHASE = 3;
  readonly attribute unsigned short eventPhase;

  undefined stopPropagation();
  attribute boolean cancelBubble;
  undefined stopImmediatePropagation();

  readonly attribute boolean bubbles;
  readonly attribute boolean cancelable;
  attribute boolean returnValue;
  undefined preventDefault();
  readonly attribute boolean defaultPrevented;
  readonly attribute boolean composed;

  readonly attribute boolean isTrusted;
  readonly attribute DOMHighResTimeStamp timeStamp;

  undefined initEvent(DOMString type, optional boolean bubbles = false, optional boolean cancelable = false);
};

// the CustomEventInit dictionary is read as any
[Exposed=*, Side]
interface CustomEvent : Event {
  constructor(DOMString type, optional any eventInitDict);

  readonly attribute any detail;

  undefined initCustomEvent(DOMString type, optional boolean bubbles = false, optional boolean cancelable = false, optional any detail = null);
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
  [SameObject] readonly attribute NodeList childNodes;
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

  [CEReactions] attribute DOMString id;
  [CEReactions] attribute DOMString className;

  boolean hasAttributes();
  sequence<DOMString> getAttributeNames();
  DOMString? getAttribute(DOMString qualifiedName);
  [CEReactions] undefined setAttribute(DOMString qualifiedName, DOMString value);
  [CEReactions] undefined removeAttribute(DOMString qualifiedName);
  // the optional boolean is read as any, to tell a missing one from false
  [CEReactions] boolean toggleAttribute(DOMString qualifiedName, optional any force);
  boolean hasAttribute(DOMString qualifiedName);

  HTMLCollection getElementsByTagName(DOMString qualifiedName);
  HTMLCollection getElementsByClassName(DOMString classNames);

  // the ChildNode and NonDocumentTypeChildNode mixins
  [CEReactions] undefined remove();
  readonly attribute Element? previousElementSibling;
  readonly attribute Element? nextElementSibling;

  // the ParentNode mixin
  [SameObject] readonly attribute HTMLCollection children;
  readonly attribute Element? firstElementChild;
  readonly attribute Element? lastElementChild;
  readonly attribute unsigned long childElementCount;
  Element? querySelector(DOMString selectors);
  [NewObject] NodeList querySelectorAll(DOMString selectors);
};

[Exposed=Window]
interface Document : Node {
  readonly attribute Element? documentElement;

  [NewObject] Element createElement(DOMString localName);
  [NewObject] DocumentFragment createDocumentFragment();
  [NewObject] Text createTextNode(DOMString data);
  [NewObject] Comment createComment(DOMString data);

  Element? getElementById(DOMString elementId);
  HTMLCollection getElementsByTagName(DOMString qualifiedName);
  HTMLCollection getElementsByClassName(DOMString classNames);

  // the legacy interface names of Event only, until more event
  // interfaces are bound
  [NewObject] Event createEvent(DOMString interface);

  // the ParentNode mixin
  [SameObject] readonly attribute HTMLCollection children;
  readonly attribute Element? firstElementChild;
  readonly attribute Element? lastElementChild;
  readonly attribute unsigned long childElementCount;
  Element? querySelector(DOMString selectors);
  [NewObject] NodeList querySelectorAll(DOMString selectors);

  // one string, not the standard's variadic list, until variadic
  // arguments are bound
  [CEReactions] undefined write(DOMString text);
};

[Exposed=Window]
interface DocumentFragment : Node {
  constructor();

  // the NonElementParentNode and ParentNode mixins
  Element? getElementById(DOMString elementId);
  [SameObject] readonly attribute HTMLCollection children;
  readonly attribute Element? firstElementChild;
  readonly attribute Element? lastElementChild;
  readonly attribute unsigned long childElementCount;
  Element? querySelector(DOMString selectors);
  [NewObject] NodeList querySelectorAll(DOMString selectors);
};

[Exposed=Window]
interface CharacterData : Node {
  attribute [LegacyNullToEmptyString] DOMString data;
  readonly attribute unsigned long length;
  DOMString substringData(unsigned long offset, unsigned long count);
  undefined appendData(DOMString data);
  undefined insertData(unsigned long offset, DOMString data);
  undefined deleteData(unsigned long offset, unsigned long count);
  undefined replaceData(unsigned long offset, unsigned long count, DOMString data);

  // the ChildNode and NonDocumentTypeChildNode mixins
  [CEReactions] undefined remove();
  readonly attribute Element? previousElementSibling;
  readonly attribute Element? nextElementSibling;
};

[Exposed=Window]
interface Text : CharacterData {
  constructor(optional DOMString data = "");

  [NewObject] Text splitText(unsigned long offset);
  readonly attribute DOMString wholeText;
};

[Exposed=Window]
interface Comment : CharacterData {
  constructor(optional DOMString data = "");
};

// [Side]: a collection is a side object; one from querySelectorAll is
// fixed, the others are live
[Exposed=Window, Side]
interface NodeList {
  getter Node? item(unsigned long index);
  readonly attribute unsigned long length;
  iterable<Node>;
};

// named properties are not intercepted yet: namedItem only
[Exposed=Window, Side]
interface HTMLCollection {
  readonly attribute unsigned long length;
  getter Element? item(unsigned long index);
  Element? namedItem(DOMString name);
};
