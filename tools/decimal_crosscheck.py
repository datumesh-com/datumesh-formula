#!/usr/bin/env python3
"""Cross-check of the crate's decimal arithmetic (SPEC 15.9) against Python's `decimal`.

    python3 tools/decimal_crosscheck.py generate N SEED > cases.jsonl
    python3 tools/decimal_crosscheck.py python cases.jsonl > python.jsonl      # Python >= 3.13
    python3 tools/decimal_crosscheck.py compare cases.jsonl crate.jsonl python.jsonl
    python3 tools/decimal_crosscheck.py trig N SEED > trig.jsonl              # formula-eval vectors

`generate` writes one `decimal` vector (SPEC 11) per operation with N random cases: add, sub,
mul, div, power (integer exponents), sqrt, exp, ln and pow (non-integer exponents). `python`
computes each case with Python's decimal in SPEC 15.9's context (prec 34, ROUND_HALF_EVEN,
Emin -6143, Emax 6144, clamp 0, InvalidOperation, DivisionByZero and Overflow trapped) plus the
range rule and the domains of 15.9.2-15.9.3. `compare` requires + - * / and integer powers to
agree exactly and sqrt, exp, ln and pow to agree within one unit in the 34th digit, counting the
cases that differ by that unit (hard cases: the algorithm wins, 15.9.3). `trig` writes
formula-eval vectors for sin, cos, atan2 and haversine, which have no Python decimal
counterpart; run them through the crate and through the reference tool as a black box.

Standard library only; it does not use the reference's fdecimal.py.
"""
import decimal
import json
import random
import sys
from decimal import Decimal

sys.set_int_max_str_digits(0)

OPS = ['add', 'sub', 'mul', 'div', 'power', 'sqrt', 'exp', 'ln', 'pow']


def canonical(d):
    """The canonical lexical form of SPEC 3.1."""
    if d.is_zero():
        return '0'
    s = format(d, 'f')  # exact: no context rounding
    if '.' in s:
        s = s.rstrip('0').rstrip('.')
    return s


def rand_number(rng, digits=None, emin=-30, emax=30, sign=True):
    n = digits or rng.randrange(1, 41)
    coef = str(rng.randrange(1, 10)) + ''.join(str(rng.randrange(10)) for _ in range(n - 1))
    d = Decimal(coef).scaleb(rng.randrange(emin, emax + 1) - (n - 1))
    if sign and rng.random() < 0.5:
        d = -d
    s = canonical(d)
    return s if len(s) <= 128 else rand_number(rng, 5, -5, 5, sign)


def gen_case(rng, op):
    if op in ('add', 'sub', 'mul', 'div'):
        a = rand_number(rng)
        b = rand_number(rng) if rng.random() < 0.97 else '0'
        if op in ('add', 'sub') and rng.random() < 0.2:
            b = canonical(Decimal(a).scaleb(-rng.randrange(0, 40)))
        return [a, b]
    if op == 'power':
        x = rand_number(rng, rng.randrange(1, 20), -5, 5)
        n = rng.randrange(-60, 61) if rng.random() < 0.95 else rng.choice([1024, -1024, 1025, 0, 300])
        return [x, str(n)]
    if op == 'sqrt':
        return [rand_number(rng, sign=rng.random() < 0.03)]
    if op == 'exp':
        x = Decimal(rand_number(rng, rng.randrange(1, 41), -20, 2))
        if rng.random() < 0.7:
            x = x.copy_abs() % 300 * (1 if rng.random() < 0.5 else -1)
        return [canonical(x)]
    if op == 'ln':
        return [rand_number(rng, sign=rng.random() < 0.02, emin=-120, emax=120)]
    if op == 'pow':
        x = rand_number(rng, rng.randrange(1, 30), -6, 6, sign=rng.random() < 0.02)
        y = Decimal(rand_number(rng, rng.randrange(1, 20), -6, 1))
        if y == y.to_integral_value():
            y += Decimal('0.5')
        return [x, canonical(y)]
    raise ValueError(op)


def generate(n, seed):
    rng = random.Random(seed)
    for op in OPS:
        cases = [gen_case(rng, op) for _ in range(n)]
        print(json.dumps({'id': 'crosscheck/' + op, 'kind': 'decimal', 'description': 'random cases', 'valid': True,
                          'op': op, 'cases': cases}))


def context():
    c = decimal.Context(prec=34, rounding=decimal.ROUND_HALF_EVEN, Emin=-6143, Emax=6144, clamp=0)
    c.traps[decimal.InvalidOperation] = True
    c.traps[decimal.DivisionByZero] = True
    c.traps[decimal.Overflow] = True
    return c


def ranged(d):
    s = canonical(d)
    return 'E_FORMULA_RANGE' if len(s) > 128 else s


def python_one(op, args):
    c = context()
    x = [Decimal(a) for a in args]
    try:
        if op == 'add':
            return ranged(c.add(x[0], x[1]))
        if op == 'sub':
            return ranged(c.subtract(x[0], x[1]))
        if op == 'mul':
            return ranged(c.multiply(x[0], x[1]))
        if op == 'div':
            if x[1].is_zero():
                return 'E_FORMULA_DIV_ZERO'
            return ranged(c.divide(x[0], x[1]))
        if op == 'power':
            n = int(x[1])
            if abs(n) > 1024:
                return 'E_FORMULA_DOMAIN'
            if n == 0:
                return '1'
            if x[0].is_zero():
                return 'E_FORMULA_DIV_ZERO' if n < 0 else '0'
            # The exact power with Python integers, rounded once.
            sign, digits, exp = x[0].as_tuple()
            coef = int(''.join(map(str, digits)))
            exact = Decimal((0, tuple(map(int, str(coef ** abs(n)))), exp * abs(n)))
            if sign and abs(n) % 2:
                exact = exact.copy_negate()
            if len(canonical(exact)) > 100000:
                return 'E_FORMULA_RANGE'
            r = c.plus(exact) if n > 0 else c.divide(Decimal(1), exact)
            return ranged(r)
        if op == 'sqrt':
            if x[0] < 0:
                return 'E_FORMULA_DOMAIN'
            return ranged(c.sqrt(x[0]))
        if op == 'exp':
            if abs(x[0]) > 300:
                return 'E_FORMULA_RANGE'
            return ranged(c.exp(x[0]))
        if op == 'ln':
            if x[0] <= 0:
                return 'E_FORMULA_DOMAIN'
            return ranged(c.ln(x[0]))
        if op == 'pow':
            if x[0].is_zero():
                return 'E_FORMULA_DIV_ZERO' if x[1] < 0 else '0'
            if x[0] < 0:
                return 'E_FORMULA_DOMAIN'
            # The exponent of EXP-T48: abs(y * ln x) <= 300.
            if abs(x[1] * x[0].ln(decimal.Context(prec=50))) > 300:
                return 'E_FORMULA_RANGE'
            return ranged(c.power(x[0], x[1]))
    except decimal.Overflow:
        return 'E_FORMULA_RANGE'
    except decimal.DivisionByZero:
        return 'E_FORMULA_DIV_ZERO'
    except decimal.InvalidOperation:
        return 'E_FORMULA_DOMAIN'
    raise ValueError(op)


def python(path):
    for line in open(path):
        v = json.loads(line)
        print(json.dumps({'results': [python_one(v['op'], a) for a in v['cases']]}))


def ulp_close(a, b):
    """Whether a and b (canonical numbers) differ by at most one unit in b's 34th digit."""
    da, db = Decimal(a), Decimal(b)
    if da == db:
        return True
    ref = db if not db.is_zero() else da
    unit = Decimal(1).scaleb(ref.adjusted() - 33)
    return abs(da - db) <= unit


def compare(cases_path, crate_path, python_path):
    total = bad = 0
    report = []
    for c, a, b in zip(open(cases_path), open(crate_path), open(python_path)):
        c, a, b = json.loads(c), json.loads(a), json.loads(b)
        op = c['op']
        n = mismatches = hard = 0
        for args, x, y in zip(c['cases'], a['results'], b['results']):
            n += 1
            if x == y:
                continue
            exact = op in ('add', 'sub', 'mul', 'div', 'power')
            if not exact and not x.startswith('E_') and not y.startswith('E_') and ulp_close(x, y):
                hard += 1
                continue
            mismatches += 1
            if mismatches <= 5:
                print('MISMATCH %s%s: crate %s, python %s' % (op, args, x, y))
        total += n
        bad += mismatches
        report.append('%-6s %6d cases, %d mismatches, %d hard cases (one unit in the 34th digit)' % (op, n, mismatches, hard))
    print('\n'.join(report))
    print('total %d cases, %d mismatches' % (total, bad))
    return 1 if bad else 0


def trig(n, seed):
    """formula-eval vectors for sin, cos, atan2 and haversine over random arguments."""
    rng = random.Random(seed)
    descriptor = lambda body, args, ret: {'code': {'body': body + '\n', 'lang': 'formula'}, 'inputs': [],
                                          'signature': {'args': args, 'order': sorted(args), 'returns': ret}}
    one = {'type': 'number', 'unit': '1', 'valid': False}
    deg = {'type': 'number', 'unit': 'deg', 'valid': False}
    for op in ('sin', 'cos', 'atan2', 'haversine'):
        for i in range(n):
            if op in ('sin', 'cos'):
                x = canonical(Decimal(rand_number(rng, rng.randrange(1, 35), -12, 3)))
                if rng.random() < 0.3:
                    x = canonical(Decimal(x) % 1000000)
                args, vals, body = {'x': one}, {'x': x}, '%s(x)' % op
                ret = {'type': 'number', 'unit': '1'}
            elif op == 'atan2':
                y, x = rand_number(rng, rng.randrange(1, 35), -10, 10), rand_number(rng, rng.randrange(1, 35), -10, 10)
                if rng.random() < 0.05:
                    y = '0'
                args, vals, body = {'x': one, 'y': one}, {'x': x, 'y': y}, 'atan2(y, x)'
                ret = {'type': 'number', 'unit': '1'}
            else:
                vals = {'a': canonical(Decimal(rng.randrange(-900000, 900001)) / 10000),
                        'b': canonical(Decimal(rng.randrange(-1800000, 1800001)) / 10000),
                        'c': canonical(Decimal(rng.randrange(-900000, 900001)) / 10000),
                        'e': canonical(Decimal(rng.randrange(-1800000, 1800001)) / 10000)}
                args, body = {k: deg for k in vals}, 'haversine(a, b, c, e)'
                ret = {'type': 'number', 'unit': 'm'}
            v = {'id': 'crosscheck/%s-%d' % (op, i), 'kind': 'formula-eval', 'description': 'random case', 'valid': True,
                 'world': {'functions': {'calc.example.net/e': {'bound': '2026-01-01T00:00:00.000000Z',
                                                                 'descriptor': descriptor(body, args, ret)}}, 'receipts': []},
                 'cell': {'fn': 'calc.example.net/e', 'args': {k: {'type': 'number', 'value': s} for k, s in vals.items()}},
                 'asof': '2026-06-01T00:00:00.000000Z'}
            print(json.dumps(v))


if __name__ == '__main__':
    cmd = sys.argv[1]
    if cmd == 'generate':
        generate(int(sys.argv[2]), int(sys.argv[3]))
    elif cmd == 'python':
        python(sys.argv[2])
    elif cmd == 'compare':
        sys.exit(compare(sys.argv[2], sys.argv[3], sys.argv[4]))
    elif cmd == 'trig':
        trig(int(sys.argv[2]), int(sys.argv[3]))
    else:
        raise SystemExit(__doc__)
