#!/usr/bin/env python3
"""Generate crates/madcawl-net/src/psl/table.rs from the public suffix list.

usage: tools/gen-psl.py crates/madcawl-net/psl > crates/madcawl-net/src/psl/table.rs

The directory holds public_suffix_list.dat and SOURCE, which records the
upstream commit and its date; tools/update-psl.sh writes both.

Every rule of both sections (ICANN and PRIVATE) becomes one entry keyed
by its domain in ASCII form (non-ASCII labels as Punycode), with flags
for what the list says about it: a plain rule, a `*.` rule for its
children, a `!` exception. The keys are sorted bytewise for binary
search. The upstream commit and its date are kept as constants.
Exits non-zero on a rule it cannot read, so a bad download is caught.
"""
import re
import sys
import unicodedata

RULE, WILDCARD, EXCEPTION = 1, 2, 4
LABEL = re.compile(r"^[a-z0-9_-]+$")


def ascii_label(label):
    label = unicodedata.normalize("NFC", label.lower())
    if label.isascii():
        return label
    return "xn--" + label.encode("punycode").decode("ascii")


def fail(n, line, why):
    sys.exit(f"gen-psl: line {n}: {line!r}: {why}")


def read(path):
    flags, sections = {}, set()
    for n, raw in enumerate(open(path, encoding="utf-8"), 1):
        line = raw.strip()
        m = re.match(r"^// ===(BEGIN|END) (ICANN|PRIVATE) DOMAINS===$", line)
        if m:
            sections.add(f"{m.group(1)} {m.group(2)}")
        if not line or line.startswith("//"):
            continue
        rule = line.split()[0]
        kind = RULE
        if rule.startswith("!"):
            kind, rule = EXCEPTION, rule[1:]
        elif rule.startswith("*."):
            kind, rule = WILDCARD, rule[2:]
        labels = [ascii_label(l) for l in rule.split(".")]
        if not all(LABEL.match(l) for l in labels):
            fail(n, line, "not a domain rule (a `*` only leads a rule)")
        key = ".".join(labels)
        flags[key] = flags.get(key, 0) | kind
    if len(sections) != 4:
        fail(0, path, "the ICANN and PRIVATE section markers are not all there")
    if len(flags) < 5000:
        fail(0, path, f"only {len(flags)} rules")
    return flags


def source(path):
    fields = dict(l.rstrip("\n").split(": ", 1) for l in open(path, encoding="utf-8") if ": " in l)
    return fields["commit"], fields["commit-date"]


def main():
    flags = read(f"{sys.argv[1]}/public_suffix_list.dat")
    commit, date = source(f"{sys.argv[1]}/SOURCE")
    out = sys.stdout
    out.write("// CODEGEN: tools/gen-psl.py from crates/madcawl-net/psl/public_suffix_list.dat\n")
    out.write("//! The public suffix list's rules, keyed by domain in ASCII form and\n")
    out.write("//! sorted bytewise.\n\n")
    out.write("/// the upstream commit's date\n")
    out.write(f"pub const VERSION: &str = \"{date}\";\n")
    out.write(f"pub const COMMIT: &str = \"{commit}\";\n\n")
    out.write("pub const RULE: u8 = 1;\n")
    out.write("/// `*.key`: every child of the key is a suffix\n")
    out.write("pub const WILDCARD: u8 = 2;\n")
    out.write("/// `!key`: the key is not a suffix though a wildcard says so\n")
    out.write("pub const EXCEPTION: u8 = 4;\n\n")
    out.write(f"pub static RULES: [(&str, u8); {len(flags)}] = [\n")
    for key in sorted(flags, key=lambda k: k.encode()):
        out.write(f"    (\"{key}\", {flags[key]}),\n")
    out.write("];\n")


main()
