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
  // dictionary only capture and once are acted on; [Atom] (not in the
  // standard) takes the type as an atom
  undefined addEventListener([Atom] DOMString type, EventListener? callback, optional any options);
  undefined removeEventListener([Atom] DOMString type, EventListener? callback, optional any options);
  boolean dispatchEvent(Event event);
};

// from Web IDL; its prototype object inherits from Object.prototype,
// not Error.prototype, until an interface may name a built-in parent
[Exposed=*, Side]
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
  readonly attribute DOMString className;

  DOMString? getAttribute(DOMString qualifiedName);
  boolean hasAttribute(DOMString qualifiedName);
  [CEReactions] undefined setAttribute(DOMString qualifiedName, DOMString value);

  // the ChildNode mixin
  [CEReactions] undefined remove();

  // the ParentNode mixin; [StaticList] (not in the standard) marks a
  // list made when called, an array, until NodeList and HTMLCollection
  // are bound: getElementsByTagName's is not live
  Element? querySelector(DOMString selectors);
  [StaticList] NodeList querySelectorAll(DOMString selectors);
  [StaticList] HTMLCollection getElementsByTagName(DOMString qualifiedName);
};

[Exposed=Window]
interface Document : Node {
  readonly attribute Element? documentElement;

  [NewObject] Element createElement(DOMString localName);
  [NewObject] DocumentFragment createDocumentFragment();
  [NewObject] Text createTextNode(DOMString data);
  [NewObject] Comment createComment(DOMString data);

  Element? getElementById(DOMString elementId);

  // the legacy interface names of Event only, until more event
  // interfaces are bound
  [NewObject] Event createEvent(DOMString interface);

  Element? querySelector(DOMString selectors);
  [StaticList] NodeList querySelectorAll(DOMString selectors);
  [StaticList] HTMLCollection getElementsByTagName(DOMString qualifiedName);

  // one string, not the standard's variadic list, until variadic
  // arguments are bound
  [CEReactions] undefined write(DOMString text);
};

[Exposed=Window]
interface DocumentFragment : Node {
  constructor();

  // the NonElementParentNode and ParentNode mixins, in part
  Element? getElementById(DOMString elementId);
  Element? querySelector(DOMString selectors);
  [StaticList] NodeList querySelectorAll(DOMString selectors);
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

  // the ChildNode mixin
  [CEReactions] undefined remove();
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
