#!/usr/bin/env python3
"""Write tests/vectors-local/class-seq-*.json: `formula-eval` vectors over a small class world
(`calc.example.net/seq`, a class with one number key `n`, whose method `next` returns a key of the
same class and whose method `val` returns a number), for chains of dispatches and the dispatch key
fault (SPEC 16.6). Their expected results are the reference tool's (tools/formula.py of a
datumesh-spec checkout at 6d5633e, run as a black box), not this library's.

    python3 tools/gen_class_vectors.py /path/to/datumesh-spec

Needs Python >= 3.13 and PyNaCl (the reference's own requirements). The vectors have the shape of
the spec's `formula-eval` vectors; the stored receipts' signatures are not checked by the
evaluators (zeros).
"""
import copy
import hashlib
import json
import os
import sys

sys.path.insert(0, sys.argv[1] + '/tools')
sys.setrecursionlimit(100000)
import formula  # noqa: E402

OUT = os.path.join(os.path.dirname(os.path.abspath(__file__)), '..', 'tests', 'vectors-local')
CL = 'calc.example.net/seq'
T = '2026-03-16T16:00:00.000000Z'
B = '2026-01-01T00:00:00.000000Z'
N = {'type': 'number', 'unit': '1', 'valid': False}


def jcs(x):
    return json.dumps(x, sort_keys=True, separators=(',', ':'), ensure_ascii=False)


def h(x):
    return hashlib.sha256(jcs(x).encode()).hexdigest()


next_f = {'code': {'body': '', 'lang': 'stored'}, 'inputs': [],
          'signature': {'args': {'n': N}, 'order': ['n'], 'returns': {'type': 'string', 'classes': [CL]}}}
val_f = {'code': {'body': '', 'lang': 'stored'}, 'inputs': [],
         'signature': {'args': {'n': N}, 'order': ['n'], 'returns': {'type': 'number', 'unit': '1'}}}
hn, hv = h(next_f), h(val_f)
cls = {'keys': {'n': {'type': 'number', 'unit': '1'}}, 'methods': {'next': hn, 'val': hv}, 'order': ['n'],
       'steward': {'namespace': 'calc.example.net', 'process': 'https://calc.example.net/classes/change'}}
SIG = {'args': {'n': N}, 'order': ['n'], 'returns': {'type': 'number', 'unit': '1'}}
INPUTS = [CL + '/next', CL + '/val']


def receipt(fn, fnh, n, typ, val):
    return {'asof': T, 'cell': {'args': {'n': {'type': 'number', 'value': str(n)}}, 'fn': fn, 'fn_hash': fnh, 'ns': CL},
            'computed_by': 'calc.example.net#0123456789abcdef', 'inputs': [], 'issued': T, 'licence': 'CC0-1.0',
            'proto': 'datumesh/1', 'publisher': CL, 'summary': 'A recorded value.', 'tx': T, 'type': typ,
            'valid': {}, 'value': val, 'sig': '0' * 128}


def vector(vid, description, body, next1, n=1):
    receipts = [receipt('next', hn, 1, 'string', next1), receipt('next', hn, 2, 'string', 'calc.example.net/seq(n:3)'),
                receipt('val', hv, 2, 'number', '42'), receipt('val', hv, 3, 'number', '7')]
    fd = {'code': {'body': body, 'lang': 'formula'}, 'inputs': INPUTS, 'signature': SIG}
    return {
        'id': vid, 'kind': 'formula-eval', 'description': description, 'valid': True,
        'world': {'functions': {CL + '/next': {'descriptor': next_f, 'bound': B}, CL + '/val': {'descriptor': val_f, 'bound': B},
                                'calc.example.net/two_ahead': {'descriptor': fd, 'bound': B}},
                  'classes': {CL: {'descriptor': cls, 'bound': B}}, 'receipts': receipts},
        'cell': {'fn': 'calc.example.net/two_ahead', 'args': {'n': {'type': 'number', 'value': str(n)}}},
        'asof': '2026-06-01T00:00:00.000000Z',
    }


GOOD = 'calc.example.net/seq(n:2)'
CASES = [
    ('class-seq-chain', 'next().val(): the dispatch on the key next returns; lineage lists both reads.',
     'calc.example.net/seq/next(n: n).val()\n', GOOD),
    ('class-seq-chain-twice', 'next().next().val(): a chain of two dispatches over the returned class.',
     'calc.example.net/seq/next(n: n).next().val()\n', GOOD),
    ('class-seq-no-value', 'The dispatched read has no value: plain not_covered, not a fault.',
     'calc.example.net/seq/next(n: n).val()\n', 'calc.example.net/seq(n:9)'),
]
for tag, why, key in [
    ('leading-zero', 'a number key not in its lexical form', 'calc.example.net/seq(n:02)'),
    ('not-canonical-number', 'a number key not canonical', 'calc.example.net/seq(n:2.0)'),
    ('wrong-key-name', 'a key name the class does not have', 'calc.example.net/seq(m:2)'),
    ('other-class', 'the key of a class outside the set', 'calc.example.net/other(n:2)'),
    ('not-an-encoding', 'a string that is no encoding', 'garbage'),
    ('trailing-space', 'text after the closing parenthesis', 'calc.example.net/seq(n:2) '),
    ('string-typed', 'a number key written as a string', 'calc.example.net/seq(n:"2")'),
    ('empty', 'the empty string', ''),
]:
    CASES.append(('class-seq-key-' + tag, 'The returned key is ' + why + ': the fault E_FORMULA_DISPATCH_KEY.',
                  'calc.example.net/seq/next(n: n).val()\n', key))

os.makedirs(OUT, exist_ok=True)
for vid, why, body, key in CASES:
    v = vector('local/' + vid, why, body, key)
    r = formula.run_vector(copy.deepcopy(v))
    r.pop('detail', None)
    if r.get('error'):
        v['valid'] = False
        v['error'] = r['error']
    else:
        v.update({k: r[k] for k in ('value', 'lineage', 'tx', 'receipt_asof', 'fuel_used', 'receipt_hash')})
    with open(os.path.join(OUT, vid + '.json'), 'w') as f:
        json.dump(v, f, sort_keys=True, indent=1, ensure_ascii=False)
        f.write('\n')
    print(vid, r.get('error') or r['value'])
