// The XMLHttpRequest Standard, asynchronous requests only: open with
// async false throws an InvalidAccessError, where the standard would
// block the window until the response came (a window here never waits
// on the network). Methods and header names and values the standard
// types ByteString are DOMStrings checked by the implementation, as is
// open's async; open's username and password are not bound yet (given,
// they are ignored).
// upload, responseXML and the "document" response type's documents are
// not bound yet: responseType may be set to "document", and response is
// then null. The body of send is any, converted as XMLHttpRequestBodyInit
// by the implementation: a Blob, a buffer source, URLSearchParams, and
// anything else as a USVString
[Exposed=*, Side]
interface XMLHttpRequestEventTarget : EventTarget {
  attribute EventHandler onloadstart;
  attribute EventHandler onprogress;
  attribute EventHandler onabort;
  attribute EventHandler onerror;
  attribute EventHandler onload;
  attribute EventHandler ontimeout;
  attribute EventHandler onloadend;
};

[Exposed=Window, Side]
interface XMLHttpRequest : XMLHttpRequestEventTarget {
  constructor();

  attribute EventHandler onreadystatechange;

  const unsigned short UNSENT = 0;
  const unsigned short OPENED = 1;
  const unsigned short HEADERS_RECEIVED = 2;
  const unsigned short LOADING = 3;
  const unsigned short DONE = 4;
  readonly attribute unsigned short readyState;

  undefined open(DOMString method, USVString url, optional any async);
  undefined setRequestHeader(DOMString name, DOMString value);
  attribute unsigned long timeout;
  attribute boolean withCredentials;
  [RunsScript] undefined send(optional any body = null);
  [RunsScript] undefined abort();

  readonly attribute USVString responseURL;
  readonly attribute unsigned short status;
  readonly attribute DOMString statusText;
  DOMString? getResponseHeader(DOMString name);
  DOMString getAllResponseHeaders();
  undefined overrideMimeType(DOMString mime);
  attribute DOMString responseType;
  readonly attribute any response;
  readonly attribute DOMString responseText;
};

// the ProgressEventInit dictionary is read as any
[Exposed=*, Side]
interface ProgressEvent : Event {
  constructor(DOMString type, optional any eventInitDict);
  readonly attribute boolean lengthComputable;
  readonly attribute unsigned long long loaded;
  readonly attribute unsigned long long total;
};
