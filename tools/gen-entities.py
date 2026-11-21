#!/usr/bin/env python3
"""Generate crates/madcawl-html/src/entities.rs from the WHATWG entities.json.

usage: tools/gen-entities.py path/to/entities.json > crates/madcawl-html/src/entities.rs
The JSON is https://html.spec.whatwg.org/entities.json.
"""
import json
import sys

data = json.load(open(sys.argv[1]))
items = sorted((k[1:].encode(), v["codepoints"]) for k, v in data.items())
out = sys.stdout
out.write("// CODEGEN: tools/gen-entities.py from the WHATWG entities.json\n")
out.write("//! Named character references, sorted by name for prefix search.\n\n")
out.write(f"pub(crate) static NAMES: [&[u8]; {len(items)}] = [\n")
for name, _ in items:
    out.write(f"    b\"{name.decode()}\",\n")
out.write("];\n\n")
out.write("/// The one or two code points each name stands for; 0 when only one.\n")
out.write(f"pub(crate) static CHARS: [(u32, u32); {len(items)}] = [\n")
for _, cps in items:
    a = cps[0]
    b = cps[1] if len(cps) > 1 else 0
    out.write(f"    (0x{a:X}, 0x{b:X}),\n")
out.write("];\n")
