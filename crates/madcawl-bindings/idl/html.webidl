// The HTML Standard's script element. It inherits from Element directly
// until HTMLElement is bound, and carries the onload and onerror event
// handlers that HTMLElement's GlobalEventHandlers would give it.
// noModule, charset, crossOrigin, integrity and referrerPolicy are not
// bound yet.

[Exposed=Window]
interface HTMLScriptElement : Element {
  [CEReactions, RunsScript] attribute DOMString src;
  [CEReactions] attribute DOMString type;
  [CEReactions] attribute boolean async;
  [CEReactions] attribute boolean defer;
  [CEReactions, RunsScript] attribute DOMString text;
  attribute EventHandler onload;
  attribute EventHandler onerror;
};
