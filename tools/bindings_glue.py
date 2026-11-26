"""The Rust glue tools/gen-bindings.py writes for each member: a function
generic over the engine that converts `this` and calls the member's
implementation, and the C-ABI shell its table holds; and the IDL types
and effects in the signature of each table entry."""
import re
import sys

KEYWORDS = {'type', 'match', 'move', 'ref', 'loop', 'fn', 'use', 'mod', 'impl', 'self', 'super', 'where', 'as'}


def snake(name):
    name = re.sub(r'(?<=[A-Z])([A-Z])(?=[a-z])', r'_\1', name)
    return re.sub(r'(?<=[a-z0-9])([A-Z])', r'_\1', name).lower()


def ident(name):
    s = snake(name)
    return 'r#' + s if s in KEYWORDS else s


def glue(fn, iface, body, kind, uses_args=True):
    head = {
        'ctor': f'fn {fn}<E: Engine>(e: &mut E, h: &mut DomHost<E>, _this: E::Value, {"args" if uses_args else "_args"}: &[E::Value]) -> Owned<E::Value> {{',
        'op': f'fn {fn}<E: Engine>(e: &mut E, h: &mut DomHost<E>, this: E::Value, {"args" if uses_args else "_args"}: &[E::Value]) -> Owned<E::Value> {{',
        'get': f'fn {fn}<E: Engine>(e: &mut E, h: &mut DomHost<E>, this: E::Value) -> Owned<E::Value> {{',
        'set': f'fn {fn}<E: Engine>(e: &mut E, h: &mut DomHost<E>, this: E::Value, value: E::Value) -> Owned<E::Value> {{',
    }[kind]
    lines = [head, '    cv::run(e, h, |e, h| {']
    if kind != 'ctor':
        kind_of = 'side' if iface['side'] else 'target' if iface['targets'] else 'node'
        lines.append(f"        let this = cv::this_{kind_of}(e, h, this, &{iface['name']}Class::<E>::DEF)?;")
    lines += ['        ' + b for b in body]
    lines += ['    })', '}', '']
    call = {
        'ctor': f'shell::op(ctx, this, argv, argc, {fn}::<E>)', 'op': f'shell::op(ctx, this, argv, argc, {fn}::<E>)',
        'get': f'shell::get(ctx, this, {fn}::<E>)',
        'set': f'shell::set(ctx, this, argv, argc, {fn}::<E>)',
    }[kind]
    words = '_: *const u64, _: usize' if kind == 'get' else 'argv: *const u64, argc: usize'
    lines += ['// SAFETY: called as the host function of its table entry',
              f'unsafe extern "C" fn {fn}_abi<E: Engine>(ctx: *mut Ctx, this: u64, {words}) -> u64 {{ unsafe {{ {call} }} }}',
              '']
    return lines


STRINGS = ('DOMString', 'DOMString?', '[Atom] DOMString', '[LegacyNullToEmptyString] DOMString')
# list types a [StaticList] operation returns: the implementation gives a
# `cv::StaticList`, made an array of wrappers
LISTS = ('NodeList', 'HTMLCollection', 'sequence<EventTarget>')


def idl_type(t, names):
    if t in STRINGS:
        return 'NullableString' if t.endswith('?') else 'String'
    if t in ('undefined', 'boolean', 'any'):
        return t.capitalize()
    if t in ('unsigned short', 'short', 'unsigned long', 'long', 'double', 'DOMHighResTimeStamp'):
        return 'Number'
    if t == 'EventListener?':
        return 'Callback'
    if t.rstrip('?') in names:
        return 'NullableObject' if t.endswith('?') else 'Object'
    if t in LISTS:
        return 'Object'
    sys.exit(f'unsupported IDL type: {t}')


def effects(bits):
    if not bits:
        return 'Effects::NONE'
    out = 'Effects::' + bits[0]
    for b in bits[1:]:
        out += f'.with(Effects::{b})'
    return out


def setter_effects(a):
    bits = ['THROWS', 'WRITES']
    if a['type'] in STRINGS or 'CEReactions' in a['flags']:
        bits.insert(0, 'RUNS_SCRIPT')
    return effects(bits)


def op_sig(o, names):
    bits = []
    if any(a['type'] in STRINGS for a in o['args']) or 'CEReactions' in o['flags']:
        bits.append('RUNS_SCRIPT')
    if o['args'] or 'NewObject' in o['flags']:
        bits.append('THROWS')
    if 'NewObject' in o['flags']:
        bits.append('NEW_OBJECT')
    if 'CEReactions' in o['flags']:
        bits.append('WRITES')
    args = ', '.join(f"IdlType::{idl_type(a['type'], names)}" for a in o['args'])
    return (f"Signature {{ args: &[{args}], result: IdlType::{idl_type(o['ret'], names)}, "
            f"effects: {effects(bits)} }}")
