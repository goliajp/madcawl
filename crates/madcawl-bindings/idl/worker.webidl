// Dedicated workers of the HTML Standard, classic scripts only: a
// WorkerOptions `type` of "module" throws a NotSupportedError, and a
// script URL of another origin than the window a SecurityError. The
// options are given as any. A worker runs in a realm of its own on the
// window's thread and event loop, not in parallel (a busy worker holds
// up its window); its global has `self`, `name`, `postMessage`, `close`,
// the message and error handlers, the listener methods, the timers,
// `queueMicrotask`, `atob` and `btoa`, and the interfaces exposed
// everywhere; `importScripts`, `location` and `navigator` are not bound
// yet, nor are nested workers. postMessage's transfer list is not acted
// on: buffers are copied, not detached
[Exposed=Window, Side]
interface Worker : EventTarget {
  constructor(USVString scriptURL, optional any options);
  undefined terminate();
  undefined postMessage(any message, optional any transfer);
  attribute EventHandler onmessage;
  attribute EventHandler onmessageerror;
  attribute EventHandler onerror;
};

// the MessageEventInit dictionary is read as any; source is always null
// and ports an empty array, as no MessagePort is bound yet
[Exposed=*, Side]
interface MessageEvent : Event {
  constructor(DOMString type, optional any eventInitDict);
  readonly attribute any data;
  readonly attribute USVString origin;
  readonly attribute DOMString lastEventId;
  readonly attribute any source;
  readonly attribute any ports;
};

// the ErrorEventInit dictionary is read as any
[Exposed=*, Side]
interface ErrorEvent : Event {
  constructor(DOMString type, optional any eventInitDict);
  readonly attribute DOMString message;
  readonly attribute USVString filename;
  readonly attribute unsigned long lineno;
  readonly attribute unsigned long colno;
  readonly attribute any error;
};
