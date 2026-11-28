// The File API's Blob. The parts, the BlobPropertyBag and slice's
// [Clamp] long long positions are given as any and converted by the
// implementation; stream() and bytes() are not bound yet
[Exposed=*, Side]
interface Blob {
  constructor(optional any blobParts, optional any options);
  readonly attribute unsigned long long size;
  readonly attribute DOMString type;
  [NewObject] Blob slice(optional any start, optional any end, optional any contentType);
  [NewObject] any text();
  [NewObject] any arrayBuffer();
};
