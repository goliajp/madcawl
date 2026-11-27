// The HTML Standard's element interfaces. An element of the HTML
// namespace gets the interface the standard gives its name when that
// interface is bound here, HTMLElement when the standard gives one that
// is not bound yet (p, form, canvas, ...) or the name is a valid custom
// element name, and HTMLUnknownElement otherwise. Members not listed
// are not bound yet.

// hidden is (boolean or unrestricted double or DOMString)? in the
// standard: the implementation converts the value as that union does.
// nonce is HTMLOrSVGElement's, onclick, onerror and onload are three of
// GlobalEventHandlers'
[Exposed=Window]
interface HTMLElement : Element {
  [CEReactions] attribute DOMString title;
  [CEReactions] attribute DOMString lang;
  [CEReactions] attribute DOMString dir;
  [CEReactions] attribute any hidden;
  attribute DOMString nonce;
  attribute EventHandler onclick;
  attribute EventHandler onerror;
  attribute EventHandler onload;
};

[Exposed=Window]
interface HTMLUnknownElement : HTMLElement {
};

[Exposed=Window]
interface HTMLHtmlElement : HTMLElement {
};

[Exposed=Window]
interface HTMLHeadElement : HTMLElement {
};

// onerror and onload are the window's: WindowEventHandlers and the
// window-reflecting body element event handlers
[Exposed=Window]
interface HTMLBodyElement : HTMLElement {
  attribute EventHandler onerror;
  attribute EventHandler onload;
};

[Exposed=Window]
interface HTMLDivElement : HTMLElement {
};

[Exposed=Window]
interface HTMLSpanElement : HTMLElement {
};

// HTMLHyperlinkElementUtils' href only
[Exposed=Window]
interface HTMLAnchorElement : HTMLElement {
  [CEReactions] attribute DOMString target;
  [CEReactions] attribute DOMString rel;
  [CEReactions] attribute DOMString href;
};

[Exposed=Window]
interface HTMLLinkElement : HTMLElement {
  [CEReactions] attribute DOMString href;
  [CEReactions] attribute DOMString rel;
};

[Exposed=Window]
interface HTMLStyleElement : HTMLElement {
};

[Exposed=Window]
interface HTMLImageElement : HTMLElement {
  [CEReactions] attribute DOMString alt;
  [CEReactions] attribute DOMString src;
};

// contentWindow and contentDocument wait for nested browsing contexts
[Exposed=Window]
interface HTMLIFrameElement : HTMLElement {
  [CEReactions] attribute DOMString src;
  [CEReactions] attribute DOMString name;
  [CEReactions] attribute DOMString width;
  [CEReactions] attribute DOMString height;
};

[Exposed=Window]
interface HTMLObjectElement : HTMLElement {
};

// value and the other form control members wait for form control state
[Exposed=Window]
interface HTMLButtonElement : HTMLElement {
};

[Exposed=Window]
interface HTMLInputElement : HTMLElement {
};

// noModule, charset, crossOrigin, integrity and referrerPolicy are not
// bound yet
[Exposed=Window]
interface HTMLScriptElement : HTMLElement {
  [CEReactions, RunsScript] attribute DOMString src;
  [CEReactions] attribute DOMString type;
  [CEReactions] attribute boolean async;
  [CEReactions] attribute boolean defer;
  [CEReactions, RunsScript] attribute DOMString text;
};
