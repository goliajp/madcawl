#!/usr/bin/env python3
"""Generate crates/madcawl-bindings/src/generated.rs from the Web IDL in
crates/madcawl-bindings/idl/.

usage: tools/gen-bindings.py crates/madcawl-bindings/idl/*.webidl > crates/madcawl-bindings/src/generated.rs
       tools/gen-bindings.py --check crates/madcawl-bindings/src/generated.rs crates/madcawl-bindings/idl/*.webidl

For each interface: a class table (`<Name>Class::<E>::DEF`) and one glue
function per member, generic over the engine. Class ids are given in
preorder over the inheritance tree, so a class and its subclasses are
one id range. The glue converts `this` and the arguments by their IDL
types (`convert.rs`), calls the member's implementation
`impls::<interface>::<member>` (`set_<member>` for a setter), and
converts the result. The table holds a C-ABI shell per member
(`<glue>_abi`), which takes the words the engine passes and calls the
glue (`shell.rs`).

The parser reads the subset of Web IDL the files use: interfaces with
a parent, a constructor, constants, attributes, operations with
optional arguments and defaults (boolean, string literals, and
null for `any`),
nullable types, and extended attribute
lists (only `[HostOps]`, `[Side]` (its objects are not nodes),
`[Targets]` (nodes, and side objects of its own) and `[ErrorPrototype]`
(its prototype object inherits from Error.prototype) on an interface, `[Atom]` on an argument, and `[CEReactions]` and
`[NewObject]` on a member are acted on; `[StaticList]` documents a list
result).
A `[Atom] DOMString` argument is a name, such as an event type: the
implementation gets an atom made from the value, with no string block
for a short string.

Each table entry carries the member's signature (`Signature`,
`AttributeSig`): the IDL types of its arguments and result, and its
effects. An argument converted by ToString may run script; a member
with arguments may throw (too few, wrong type), as may a setter and a
`[NewObject]` operation (the node limit); `[CEReactions]` marks a member
that changes the tree and may run custom element reactions. Getters
take no arguments and do not throw once the receiver passed its brand
check.
"""
import re
import sys

from bindings_glue import glue, ident, idl_type, op_sig, setter_effects, snake

# the conversion of an argument of each IDL type; interface types are
# converted by `convert`
TYPES = {
    'DOMString': 'cv::dom_string(e, h, {v})?',
    'DOMString?': 'cv::dom_string_or_null(e, h, {v})?',
    '[Atom] DOMString': 'cv::atom(e, h, {v})?',
    '[LegacyNullToEmptyString] DOMString': 'cv::dom_string_null_empty(e, h, {v})?',
    'unsigned long': 'cv::to_uint32(e, h, {v})?',
    'EventListener?': 'cv::callback_or_null(e, {v})?',
    'boolean': 'e.to_boolean({v})',
    'any': '{v}',
}


def strip(text):
    text = re.sub(r'/\*.*?\*/', '', text, flags=re.S)
    return re.sub(r'//[^\n]*', '', text)


def parse(text):
    out = []
    for m in re.finditer(r'(\[[^\]]*\])?\s*interface\s+(\w+)\s*(?::\s*(\w+))?\s*\{(.*?)\}\s*;', strip(text), re.S):
        ext, name, parent, body = m.groups()
        iface = {'name': name, 'parent': parent, 'host_ops': bool(ext and re.search(r'\bHostOps\b', ext)),
                 'side': bool(ext and re.search(r'\bSide\b', ext)),
                 'targets': bool(ext and re.search(r'\bTargets\b', ext)),
                 'error_proto': bool(ext and re.search(r'\bErrorPrototype\b', ext)), 'consts': [], 'attrs': [], 'ops': [],
                 'ctor': None}
        for member in body.split(';'):
            ext = re.match(r'^\s*\[([^\]]*)\]', member)
            flags = set(re.findall(r'\w+', ext.group(1))) if ext else set()
            member = re.sub(r'^\s*\[[^\]]*\]', '', member).strip()
            if not member:
                continue
            if member.startswith('const '):
                c = re.fullmatch(r'const\s+(.+?)\s+(\w+)\s*=\s*(\S+)', member)
                iface['consts'].append((c.group(2), c.group(3)))
            elif re.match(r'(readonly\s+)?attribute\s', member):
                a = re.fullmatch(r'(readonly\s+)?attribute\s+(.+?)\s+(\w+)', member)
                iface['attrs'].append({'name': a.group(3), 'type': a.group(2), 'readonly': bool(a.group(1)),
                                       'flags': flags})
            else:
                ctor = member.startswith('constructor')
                o = re.fullmatch(r'(.+?)\s+(\w+)\s*\((.*)\)', 'undefined ' * ctor + member, re.S)
                args = []
                for arg in filter(None, (x.strip() for x in o.group(3).split(','))):
                    a = re.fullmatch(r'(\[\s*Atom\s*\]\s*)?(optional\s+)?(.+?)\s+(\w+)(?:\s*=\s*(\S+))?', arg)
                    args.append({'type': ('[Atom] ' if a.group(1) else '') + a.group(3), 'name': a.group(4),
                                 'optional': bool(a.group(2)), 'default': a.group(5)})
                op = {'name': o.group(2), 'ret': o.group(1), 'args': args, 'flags': flags}
                if ctor:
                    iface['ctor'] = op
                else:
                    iface['ops'].append(op)
        out.append(iface)
    return out


def number(ifaces):
    by_parent = {}
    for i in ifaces:
        by_parent.setdefault(i['parent'], []).append(i)
    names = {i['name'] for i in ifaces}
    for i in ifaces:
        if i['parent'] and i['parent'] not in names:
            sys.exit(f"{i['name']}: unknown parent {i['parent']}")
    next_id = [1]

    def walk(i):
        i['id'] = next_id[0]
        next_id[0] += 1
        for c in by_parent.get(i['name'], []):
            walk(c)
        i['last'] = next_id[0] - 1
    for root in by_parent.get(None, []):
        walk(root)
    return names


def convert(t, v, names, sides=()):
    if t in TYPES:
        return TYPES[t].format(v=v)
    base = t.rstrip('?')
    if base in sides and not t.endswith('?'):
        return f'cv::side(e, h, {v}, &{base}Class::<E>::DEF)?'
    if base in names:
        f = 'cv::node_or_null' if t.endswith('?') else 'cv::node'
        return f'{f}(e, h, {v}, &{base}Class::<E>::DEF)?'
    sys.exit(f'unsupported IDL type: {t}')


def emit(ifaces, sources):
    names = number(ifaces)
    sides = {i['name'] for i in ifaces if i['side']}
    out = [f"// CODEGEN: tools/gen-bindings.py from {', '.join(sources)}",
           '//! The class tables of the bound interfaces, and the glue between',
           '//! script values and the implementations in `impls`.', '',
           'use std::marker::PhantomData;', '',
           'use madcawl_js::{', '    Attribute, AttributeSig, Bound, ClassDef, ClassId, Constant, Ctx, Effects, Engine, IdlType, Operation, Owned,',
           '    Signature,', '};', '',
           'use crate::convert as cv;', 'use crate::gc::NodeOps;', 'use crate::host::DomHost;', 'use crate::impls;',
           'use crate::shell;', 'use crate::side_ops::SideOps;', '']
    for i in sorted(ifaces, key=lambda i: i['id']):
        n, m = i['name'], snake(i['name'])
        out += [f'pub struct {n}Class<E>(PhantomData<E>);', '',
                f'impl<E: Engine> {n}Class<E> {{',
                '    // SAFETY: every shell in the table reads its context as E and DomHost<E>',
                f'    pub const DEF: ClassDef<E, DomHost<E>> = ClassDef {{',
                f'        name: "{n}",', f"        id: ClassId({i['id']}),", f"        last: ClassId({i['last']}),",
                f"        parent: {'Some(&' + i['parent'] + 'Class::<E>::DEF)' if i['parent'] else 'None'},"]
        attrs = []
        for a in i['attrs']:
            s = 'None' if a['readonly'] else f"Some(unsafe {{ Bound::host_fn({m}_set_{snake(a['name'])}_abi::<E>) }})"
            set = 'Effects::NONE' if a['readonly'] else setter_effects(a)
            sig = f"AttributeSig {{ ty: IdlType::{idl_type(a['type'], names)}, get: Effects::NONE, set: {set} }}"
            attrs.append(f"Attribute {{ name: \"{a['name']}\", get: unsafe {{ Bound::host_fn({m}_get_{snake(a['name'])}_abi::<E>) }}, set: {s}, sig: {sig} }},")
        ops = []
        for o in i['ops']:
            length = sum(1 for a in o['args'] if not a['optional'])
            ops.append(f"Operation {{ name: \"{o['name']}\", length: {length}, call: unsafe {{ Bound::host_fn({m}_{snake(o['name'])}_abi::<E>) }}, sig: {op_sig(o, names)} }},")
        consts = [f'Constant {{ name: "{c}", value: {float(int(v, 0))!r} }},' for c, v in i['consts']]
        for field, items in (('attributes', attrs), ('operations', ops), ('constants', consts)):
            if items:
                out += [f'        {field}: &['] + ['            ' + x for x in items] + ['        ],']
            else:
                out.append(f'        {field}: &[],')
        ops_of = 'Some(&SideOps::<E>::OPS)' if i['side'] or i['targets'] else 'Some(&NodeOps::<E>::OPS)' if i['host_ops'] else 'None'
        ctor = f"Some(unsafe {{ Bound::host_fn({m}_constructor_abi::<E>) }})" if i['ctor'] else 'None'
        length = sum(1 for a in i['ctor']['args'] if not a['optional']) if i['ctor'] else 0
        out += ['        interceptors: None,', f"        ops: {ops_of},", f'        constructor: {ctor},', f'        length: {length},',
                f"        error_prototype: {'true' if i['error_proto'] else 'false'},", '    };', '}', '']
        for a in i['attrs']:
            out += glue(f"{m}_get_{snake(a['name'])}", i,
                        [f"impls::{m}::{ident(a['name'])}(e, h, this)"], 'get')
            if not a['readonly']:
                out += glue(f"{m}_set_{snake(a['name'])}", i,
                            [f"let value = {convert(a['type'], 'value', names, sides)};",
                             f"impls::{m}::set_{snake(a['name'])}(e, h, this, value)"], 'set')
        members = [(o, 'op') for o in i['ops']] + ([(i['ctor'], 'ctor')] if i['ctor'] else [])
        for o, kind in members:
            body = []
            required = sum(1 for a in o['args'] if not a['optional'])
            if required:
                body.append(f'cv::need(e, args, {required})?;')
            for k, a in enumerate(o['args']):
                v = f'cv::arg::<E>(args, {k})'
                if a['default'] is not None and a['type'] == 'DOMString' and re.fullmatch(r'"[^"\\]*"', a['default']):
                    body.append(f"let a{k} = cv::dom_string_or(e, h, {v}, b{a['default']})?;")
                elif a['default'] == 'null' and a['type'] == 'any':
                    body.append(f"let a{k} = cv::any_or_null::<E>(e, {v});")
                elif a['default'] is not None:
                    if a['type'] != 'boolean' or a['default'] not in ('true', 'false'):
                        sys.exit(f"unsupported default: {a['type']} = {a['default']}")
                    body.append(f"let a{k} = cv::boolean_or(e, {v}, {a['default']});")
                else:
                    body.append(f"let a{k} = {convert(a['type'], v, names, sides)};")
            call_args = ''.join(f', a{k}' for k in range(len(o['args'])))
            this = '' if kind == 'ctor' else ', this'
            body.append(f"impls::{m}::{ident(o['name'])}(e, h{this}{call_args})")
            out += glue(f"{m}_{snake(o['name'])}", i, body, kind, bool(o['args']))
    return '\n'.join(out).rstrip('\n') + '\n'


def main(argv):
    check = None
    if argv and argv[0] == '--check':
        check, argv = argv[1], argv[2:]
    ifaces = []
    for path in argv:
        ifaces += parse(open(path, encoding='utf-8').read())
    text = emit(ifaces, argv)
    if check is None:
        sys.stdout.write(text)
        return 0
    if open(check, encoding='utf-8').read() != text:
        print(f'{check} is not what tools/gen-bindings.py makes from {" ".join(argv)}', file=sys.stderr)
        return 1
    return 0


if __name__ == '__main__':
    sys.exit(main(sys.argv[1:]))
