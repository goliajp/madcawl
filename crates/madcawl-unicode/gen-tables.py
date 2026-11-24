#!/usr/bin/env python3
"""Generate src/tables.rs from a directory of Unicode Character Database files.

usage: gen-tables.py <ucd-dir> > src/tables.rs

<ucd-dir> holds the files of one UCD version laid out as under https://www.unicode.org/Public/<version>/ucd/:
ReadMe.txt, PropertyValueAliases.txt, extracted/Derived{BidiClass,LineBreak,EastAsianWidth,GeneralCategory}.txt,
Scripts.txt, DerivedCoreProperties.txt, emoji/emoji-data.txt, auxiliary/{Grapheme,Word}BreakProperty.txt,
BidiBrackets.txt, BidiMirroring.txt, UnicodeData.txt.

Every code point maps to one record of all its properties through a three-level trie: the top 9 bits index L1,
the middle 6 bits a block of L2, the low 6 bits a block of L3. Identical blocks are stored once.
"""
import os
import re
import sys

N = 0x110000
U = sys.argv[1]


def lines(path):
    with open(os.path.join(U, path), encoding='utf-8') as f:
        yield from f


def ranges(path):
    """(first, last, fields, is_missing_default) for every data line and every @missing line"""
    for line in lines(path):
        m = re.match(r'#\s*@missing:\s*([0-9A-F]+)\.\.([0-9A-F]+)\s*;\s*(.*)$', line)
        if m:
            yield int(m.group(1), 16), int(m.group(2), 16), [p.strip() for p in m.group(3).split(';')], True
            continue
        line = line.split('#')[0].strip()
        if not line:
            continue
        f = [p.strip() for p in line.split(';')]
        r = f[0].split('..')
        yield int(r[0], 16), int(r[-1], 16), f[1:], False


def aliases():
    """property short name -> {any alias of a value: (short, long)}"""
    out = {}
    for line in lines('PropertyValueAliases.txt'):
        line = line.split('#')[0].strip()
        if not line:
            continue
        f = [p.strip() for p in line.split(';')]
        prop, short, long_ = f[0], f[1], f[2]
        if prop == 'ccc':
            short, long_ = f[2], f[3]
        table = out.setdefault(prop, {})
        for name in f[1:]:
            table[name] = (short, long_)
    return out


ALIASES = aliases()


def prop(path, key, select=None):
    """per code point short value name; @missing lines apply first, in file order, then the data lines"""
    vals = [None] * N
    missing, data = [], []
    for a, b, f, m in ranges(path):
        if select is not None:
            if len(f) < 2 or f[0] != select:
                continue
            f = f[1:]
        (missing if m else data).append((a, b, ALIASES[key][f[0]][0]))
    for a, b, v in missing + data:
        vals[a:b + 1] = [v] * (b - a + 1)
    assert None not in vals, path
    return vals


def binary(path, name):
    vals = [False] * N
    for a, b, f, m in ranges(path):
        if not m and f[0] == name:
            vals[a:b + 1] = [True] * (b - a + 1)
    return vals


def camel(name):
    return ''.join(p[:1].upper() + p[1:] for p in name.split('_'))


# enum name, property key, variant naming, doc
ENUMS = [
    ('BidiClass', 'bc', 'short', 'Bidi_Class (UAX #9)'),
    ('LineBreak', 'lb', 'short', 'Line_Break (UAX #14), before the resolution of rule LB1'),
    ('EastAsianWidth', 'ea', 'short', 'East_Asian_Width (UAX #11)'),
    ('GeneralCategory', 'gc', 'short', 'General_Category'),
    ('Script', 'sc', 'long', 'Script (UAX #24)'),
    ('GraphemeBreak', 'GCB', 'long', 'Grapheme_Cluster_Break (UAX #29)'),
    ('WordBreak', 'WB', 'long', 'Word_Break (UAX #29)'),
]
# gc group values (L, LC, ...) are not values a code point has
GC_GROUPS = {'C', 'L', 'LC', 'M', 'N', 'P', 'S', 'Z'}


def variants(key, form):
    seen = {}
    for alias, (short, long_) in ALIASES[key].items():
        if key == 'gc' and short in GC_GROUPS:
            continue
        seen[short] = camel(long_) if form == 'long' else short
    return seen


def main():
    out = []
    w = out.append
    version = re.search(r'version (\d+\.\d+\.\d+)', ''.join(lines('ReadMe.txt'))).group(1)
    columns = {
        'bc': prop('extracted/DerivedBidiClass.txt', 'bc'),
        'lb': prop('extracted/DerivedLineBreak.txt', 'lb'),
        'ea': prop('extracted/DerivedEastAsianWidth.txt', 'ea'),
        'gc': prop('extracted/DerivedGeneralCategory.txt', 'gc'),
        'sc': prop('Scripts.txt', 'sc'),
        'GCB': prop('auxiliary/GraphemeBreakProperty.txt', 'GCB'),
        'WB': prop('auxiliary/WordBreakProperty.txt', 'WB'),
    }
    incb = prop('DerivedCoreProperties.txt', 'InCB', select='InCB')
    pict = binary('emoji/emoji-data.txt', 'Extended_Pictographic')
    incb_bits = {'None': 0, 'Linker': 1, 'Consonant': 2, 'Extend': 3}
    flags = [(1 if pict[c] else 0) | (incb_bits[ALIASES['InCB'][incb[c]][1]] << 1) for c in range(N)]

    w(f'// CODEGEN: gen-tables.py from the Unicode Character Database {version}\n')
    w('//! Character property tables, generated; see gen-tables.py next to this crate\'s Cargo.toml.\n\n')
    w('use crate::Props;\n\n')
    w(f'/// The Unicode version the tables were generated from.\npub const UNICODE_VERSION: (u8, u8, u8) = '
      f'({", ".join(version.split("."))});\n\n')
    names = {}
    for enum, key, form, doc in ENUMS:
        vs = variants(key, form)
        order = sorted(vs.items(), key=lambda kv: kv[1])
        names[key] = dict(order)
        w(f'/// {doc}.\n#[allow(clippy::upper_case_acronyms)]\n')
        w('#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Debug)]\n#[repr(u8)]\n')
        w(f'pub enum {enum} {{\n')
        for short, var in order:
            w(f'    {var},\n')
        w('}\n\n')

    keys = [e[1] for e in ENUMS]
    record_of, records, ids = {}, [], []
    for c in range(N):
        r = tuple(columns[k][c] for k in keys) + (flags[c],)
        i = record_of.get(r)
        if i is None:
            i = record_of[r] = len(records)
            records.append(r)
        ids.append(i)
    l3, l2 = {}, []
    for i in range(0, N, 64):
        l2.append(l3.setdefault(tuple(ids[i:i + 64]), len(l3)))
    l2b, l1 = {}, []
    for i in range(0, len(l2), 64):
        l1.append(l2b.setdefault(tuple(l2[i:i + 64]), len(l2b)))
    assert len(records) < 65536 and len(l3) < 65536 and len(l2b) < 65536

    fields = ['bidi', 'line', 'east_asian', 'category', 'script', 'grapheme', 'word']
    w(f'pub(crate) static RECORDS: [Props; {len(records)}] = [\n')
    for r in records:
        parts = [f'{f}: {e[0]}::{names[e[1]][v]}' for f, e, v in zip(fields, ENUMS, r)]
        w(f'    Props {{ {", ".join(parts)}, flags: {r[-1]} }},\n')
    w('];\n\n')

    def array(name, ty, vals):
        w(f'pub(crate) static {name}: [{ty}; {len(vals)}] = [\n')
        for i in range(0, len(vals), 16):
            w('    ' + ', '.join(str(v) for v in vals[i:i + 16]) + ',\n')
        w('];\n\n')

    array('L1', 'u16', l1)
    array('L2', 'u16', [x for blk in sorted(l2b, key=l2b.get) for x in blk])
    array('L3', 'u16', [x for blk in sorted(l3, key=l3.get) for x in blk])

    # BD16 compares brackets under canonical equivalence; the only paired brackets with singleton
    # decompositions are U+2329 and U+232A
    canon = {}
    for line in lines('UnicodeData.txt'):
        f = line.split(';')
        if f[5] and not f[5].startswith('<') and ' ' not in f[5]:
            canon[int(f[0], 16)] = int(f[5], 16)
    brackets = []
    for a, b, f, m in ranges('BidiBrackets.txt'):
        if not m:
            pair = int(f[0], 16)
            opening = a if f[1] == 'o' else pair
            brackets.append((a, canon.get(opening, opening), f[1] == 'o'))
    w('/// (code point, canonical opening bracket of its pair, is opening), sorted by code point.\n')
    w(f'pub(crate) static BRACKETS: [(u32, u32, bool); {len(brackets)}] = [\n')
    for a, o, op in sorted(brackets):
        w(f'    (0x{a:04X}, 0x{o:04X}, {"true" if op else "false"}),\n')
    w('];\n\n')
    mirrors = sorted((a, int(f[0], 16)) for a, b, f, m in ranges('BidiMirroring.txt') if not m)
    w('/// (code point, its Bidi_Mirroring_Glyph), sorted by code point.\n')
    w(f'pub(crate) static MIRRORS: [(u32, u32); {len(mirrors)}] = [\n')
    for a, b in mirrors:
        w(f'    (0x{a:04X}, 0x{b:04X}),\n')
    w('];\n')
    sys.stdout.write(''.join(out))
    sys.stderr.write(f'unicode {version}: {len(records)} records, {len(l3)} l3 blocks, {len(l2b)} l2 blocks\n')


main()
