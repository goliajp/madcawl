// The URL Standard's URL and URLSearchParams. An optional base and the
// URLSearchParams init (a sequence of pairs, a record or a string) are
// given as any and converted by the implementation. The stringifiers
// are toString; URLSearchParams's iterable<USVString, USVString> is
// the four operations it declares, entries also being its @@iterator,
// each iterator one over the pairs as they were when it was made.
// createObjectURL takes a Blob (no MediaSource here)
[Exposed=*, Side]
interface URL {
  constructor(USVString url, optional any base);
  static URL? parse(USVString url, optional any base);
  static boolean canParse(USVString url, optional any base);
  attribute USVString href;
  readonly attribute USVString origin;
  attribute USVString protocol;
  attribute USVString username;
  attribute USVString password;
  attribute USVString host;
  attribute USVString hostname;
  attribute USVString port;
  attribute USVString pathname;
  attribute USVString search;
  [SameObject] readonly attribute URLSearchParams searchParams;
  attribute USVString hash;
  USVString toJSON();
  USVString toString();
  static DOMString createObjectURL(any obj);
  static undefined revokeObjectURL(DOMString url);
};

[Exposed=*, Side]
interface URLSearchParams {
  constructor(optional any init);
  readonly attribute unsigned long size;
  undefined append(USVString name, USVString value);
  undefined delete(USVString name, optional any value);
  USVString? get(USVString name);
  any getAll(USVString name);
  boolean has(USVString name, optional any value);
  undefined set(USVString name, USVString value);
  undefined sort();
  any entries();
  any keys();
  any values();
  undefined forEach(any callback, optional any thisArg);
  USVString toString();
};
