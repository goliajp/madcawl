// The window's own objects: Location and History of the HTML Standard,
// Crypto of the Web Cryptography API. Each window has one of each, a side
// object the window holds; members not listed are not bound yet.

// href, toString and hash read and write the document's URL; the other
// URL parts are readonly here, as setting one would navigate. The
// standard's [LegacyUnforgeable] on these members is not acted on; the
// window's `location` itself is unforgeable
[Exposed=Window, Side]
interface Location {
  attribute DOMString href;
  readonly attribute DOMString origin;
  readonly attribute DOMString protocol;
  readonly attribute DOMString host;
  readonly attribute DOMString hostname;
  readonly attribute DOMString port;
  readonly attribute DOMString pathname;
  readonly attribute DOMString search;
  attribute DOMString hash;

  undefined assign(DOMString url);
  undefined replace(DOMString url);
  undefined reload();

  // the stringifier
  DOMString toString();
};

// scrollRestoration is not bound yet; go's delta has no written default,
// as undefined converts to the standard's 0
[Exposed=Window, Side]
interface History {
  readonly attribute unsigned long length;
  readonly attribute any state;
  undefined go(optional long delta);
  undefined back();
  undefined forward();
  undefined pushState(any data, DOMString unused, optional DOMString? url);
  undefined replaceState(any data, DOMString unused, optional DOMString? url);
};

// subtle is not bound yet; getRandomValues takes and returns an
// ArrayBufferView, checked by the implementation
[Exposed=*, Side]
interface Crypto {
  any getRandomValues(any array);
  DOMString randomUUID();
};
