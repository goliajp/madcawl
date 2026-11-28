// The Encoding Standard's TextDecoder and TextEncoder. The options
// dictionaries and the buffer sources are given as any and converted by
// the implementation, as is the Uint8Array encodeInto fills; the
// TextDecodeOptions dictionary encodeInto returns is an object
[Exposed=*, Side]
interface TextDecoder {
  constructor(optional DOMString label = "utf-8", optional any options);
  readonly attribute DOMString encoding;
  readonly attribute boolean fatal;
  readonly attribute boolean ignoreBOM;
  USVString decode(optional any input, optional any options);
};

[Exposed=*, Side]
interface TextEncoder {
  constructor();
  readonly attribute DOMString encoding;
  [NewObject] any encode(optional USVString input = "");
  any encodeInto(USVString source, any destination);
};
