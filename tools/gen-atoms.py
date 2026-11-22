#!/usr/bin/env python3
"""Generate crates/madcawl-str/src/atom/generated.rs from tools/static-atoms.txt.

usage: tools/gen-atoms.py tools/static-atoms.txt > crates/madcawl-str/src/atom/generated.rs

The atoms are the names sorted bytewise; an atom's index is its place
in that order. Lookup is a perfect hash (hash and displace): the 32-bit
FNV-1a hash of the name picks a bucket by its top bits, the bucket's
displacement is mixed into the hash, and the low bits of the result
pick a slot that holds the index. The hash and the mix must stay the
same as `hash.rs` in that directory.
"""
import sys

# sections whose mixed-case names the HTML parser makes from lowercase ones
LOWERED = {"svg-elements", "svg-attributes", "mathml-attributes"}

FNV_OFFSET = 0x811C9DC5
FNV_PRIME = 0x01000193
MIX = 0x9E3779B1
M32 = 0xFFFFFFFF


def fnv(name):
    h = FNV_OFFSET
    for b in name:
        h = ((h ^ b) * FNV_PRIME) & M32
    return h


def mix(h, d):
    x = ((h ^ d) * MIX) & M32
    return x ^ (x >> 16)


def read(path):
    names = set()
    section = None
    for line in open(path, encoding="ascii"):
        line = line.strip()
        if not line or line.startswith("#"):
            continue
        if line.startswith("["):
            section = line.strip("[]")
            continue
        if section is None:
            sys.exit(f"{line}: outside any section")
        if not all(0x21 <= ord(c) < 0x7F and c not in '"\\' for c in line):
            sys.exit(f"{line!r}: a static atom is printable ASCII, no quote or backslash")
        names.add(line)
        if section in LOWERED:
            names.add(line.lower())
    return sorted(n.encode() for n in names)


def build(names):
    n = len(names)
    slot_bits = max(3, (n - 1).bit_length())
    bucket_bits = slot_bits - 2
    slots = 1 << slot_bits
    hashes = [fnv(s) for s in names]
    if len(set(hashes)) != n:
        sys.exit("two static atoms have the same hash; change FNV_OFFSET in both places")
    buckets = [[] for _ in range(1 << bucket_bits)]
    for i, h in enumerate(hashes):
        buckets[h >> (32 - bucket_bits)].append(i)
    table = [0xFFFF] * slots
    disp = [0] * len(buckets)
    order = sorted(range(len(buckets)), key=lambda b: -len(buckets[b]))
    for b in order:
        items = buckets[b]
        if not items:
            continue
        for d in range(1 << 24):
            picks = [mix(hashes[i], d) & (slots - 1) for i in items]
            if len(set(picks)) == len(picks) and all(table[p] == 0xFFFF for p in picks):
                for i, p in zip(items, picks):
                    table[p] = i
                disp[b] = d
                break
        else:
            sys.exit(f"no displacement for bucket {b}")
    return slot_bits, bucket_bits, disp, table


def rows(values, per, fmt):
    for k in range(0, len(values), per):
        yield "    " + " ".join(fmt(v) + "," for v in values[k : k + per])


def main():
    names = read(sys.argv[1])
    if len(names) >= 0xFFFF:
        sys.exit("too many static atoms for 16-bit slots")
    slot_bits, bucket_bits, disp, table = build(names)
    out = sys.stdout
    out.write("// CODEGEN: tools/gen-atoms.py from tools/static-atoms.txt\n")
    out.write("//! The static atoms and their perfect hash.\n\n")
    out.write(f"pub(crate) const COUNT: usize = {len(names)};\n\n")
    out.write(f"pub(crate) static NAMES: [&str; COUNT] = [\n")
    for s in names:
        out.write(f'    "{s.decode()}",\n')
    out.write("];\n\n")
    out.write(f"pub(crate) const SLOT_BITS: u32 = {slot_bits};\n")
    out.write(f"pub(crate) const BUCKET_BITS: u32 = {bucket_bits};\n\n")
    out.write("/// Per bucket, the value mixed into the hash before it picks a slot.\n")
    out.write(f"pub(crate) static DISPLACE: [u32; {len(disp)}] = [\n")
    out.write("\n".join(rows(disp, 8, str)) + "\n];\n\n")
    out.write("/// Per slot, the index of the atom there; `u16::MAX` when empty.\n")
    out.write(f"pub(crate) static SLOTS: [u16; {len(table)}] = [\n")
    out.write("\n".join(rows(table, 12, str)) + "\n];\n")


main()
