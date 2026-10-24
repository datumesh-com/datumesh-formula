#!/usr/bin/env python3
"""Generate random formula cases (JSON lines of `formula` and `formula-eval` vectors) for
differential runs of this crate against another implementation of SPEC 15.

    python3 tools/gen_cases.py formula N SEED ENV.json > cases.jsonl
    python3 tools/gen_cases.py eval-sources N SEED VECTOR_DIR > sources.jsonl
    (run sources.jsonl through an implementation's runner: defined.jsonl)
    python3 tools/gen_cases.py eval N SEED sources.jsonl defined.jsonl VECTOR_DIR > cases.jsonl

`formula` cases are sources over the environment ENV.json (an `env` of SPEC 11), about a third
of them mutated so that they fail somewhere. `eval` cases evaluate random bodies over a small
world of stored and computed functions. Standard library only; written for this crate, not
derived from any other implementation.
"""
import json
import random
import sys

NUM_VARS = {'a': 'EUR', 'b': 'EUR', 'c': 'USD', 'n': '1', 'k': '1', 'x': '1', 'r': 'percent',
            't1': 'degC', 't2': 'degC', 'count': 'count', 'plat': 'deg', 'plon': 'deg',
            'dist': 'm', 'secs': 's'}
STR_VARS = ['s', 'name', 'base', 'quote', 'station', 'tenor', 'item', 'stations', 'p']
UNITS = ['EUR', 'USD', '1', 'percent', 'degC', 'K', 'm', 'km', 's', 'min', 'h', 'd', 'deg', 'count', 'bp']
NUMBERS = ['0', '1', '2', '3', '10', '0.5', '1.25', '100', '0.001', '7', '12', '1000000', '3.14159',
           '99999999999999999999999999999999999', '0.1', '2.5', '360', '90', '45']


class Gen:
    def __init__(self, rng, depth):
        self.r = rng
        self.depth = depth
        self.lets = []

    def pick(self, xs):
        return self.r.choice(xs)

    def lit(self):
        n = self.pick(NUMBERS)
        if self.r.random() < 0.3:
            return n + ' ' + self.pick(UNITS)
        return n

    def num(self, d=0):
        r = self.r.random()
        if d >= self.depth or r < 0.25:
            c = self.r.random()
            if c < 0.4:
                return self.lit()
            if c < 0.9:
                return self.pick(list(NUM_VARS))
            return self.pick([n for n, _ in self.lets] or ['a'])
        k = self.r.randrange(22)
        e = lambda: self.num(d + 1)
        if k < 6:
            return '%s %s %s' % (e(), self.pick(['+', '-', '*', '/']), e())
        if k == 6:
            return '-' + self.atom(e())
        if k == 7:
            return '%s ^ %s' % (self.atom(e()), self.pick(['2', '-1', '0.5', '3', 'n', '(1 + 1)']))
        if k == 8:
            f = self.pick(['abs', 'floor', 'ceiling', 'sqrt', 'exp', 'ln', 'sin', 'cos'])
            return '%s(%s)' % (f, e())
        if k == 9:
            f = self.pick(['round', 'round_up', 'round_down'])
            return '%s(%s, %s)' % (f, e(), self.pick(['0', '1', '2', '-1', 'digits: 2']))
        if k == 10:
            return 'mod(%s, %s)' % (e(), e())
        if k == 11:
            return 'atan2(%s, %s)' % (e(), e())
        if k == 12:
            return 'if %s then %s else %s' % (self.boolean(d + 1), e(), e()) if self.r.random() < 0.5 else \
                '(if %s then %s else %s)' % (self.boolean(d + 1), e(), e())
        if k == 13:
            name = self.pick(['v1', 'w2', 'q', 'zz', 'a'])
            val = e()
            self.lets.append((name, val))
            body = e()
            self.lets.pop()
            return '(let %s = %s in %s)' % (name, val, body)
        if k == 14:
            return '%s(%s)' % (self.pick(['sum', 'mean', 'median', 'min', 'max', 'first', 'last', 'stdev', 'stdev_p', 'count']),
                               ', '.join(e() for _ in range(self.r.randrange(1, 4))))
        if k == 15:
            return '%s(%s)' % (self.pick(['sum', 'mean', 'median', 'min', 'max', 'count', 'stdev']), self.slice())
        if k == 16:
            return 'convert(%s, "%s")' % (e(), self.pick(UNITS))
        if k == 17:
            return self.pick(['len(%s)' % self.string(d + 1), 'find("a", %s)' % self.string(d + 1),
                              'year(%s)' % self.date(d + 1), 'weekday(%s)' % self.date(d + 1),
                              'year_fraction(d, d2, "%s")' % self.pick(['act/360', 'act/365f', '30/360', 'act/act_isda', '30e/360', '30/360_us'])])
        if k == 18:
            return self.pick(['rates.example.org/fx_rate(base, quote, d)', 'rates.example.org/rate(quote, tenor)',
                              'rates.example.org/fx_rate("EUR", "USD", d)', 'data.example.org/price(item: s)',
                              'data.example.org/daily(d)', '@fx(base, quote, d)', 'curve.example.net/df(tenor: 3)'])
        if k == 19:
            return self.pick(['at(data.example.org/daily(*), d)', 'last_before(data.example.org/daily(*), d, 7 d)',
                              'latest(weather.example.org/temp(station, *))', 'nearest(weather.example.org/temp(station, *), t)',
                              'interpolate(weather.example.org/temp(station, *), t)',
                              'otherwise(at(data.example.org/daily(*), d), 0)', 'first_after(weather.example.org/temp(station, *), t, 10 min)'])
        if k == 20:
            return 'haversine(%s, %s, %s, %s)' % (self.pick(['plat', '51.5', '0']), self.pick(['plon', '-0.12', '0']),
                                                  self.pick(['48.8', 'plat', '91']), self.pick(['2.35', 'plon']))
        return 'd2 - d' if self.r.random() < 0.5 else 't - instant(d)'

    def atom(self, s):
        if all(ch.isalnum() or ch in '_.' for ch in s.replace(' ', '')) and ' ' not in s:
            return s
        return '(' + s + ')'

    def slice(self):
        return self.pick(['data.example.org/daily(*)', 'data.example.org/daily([2024-01-01..2024-12-31])',
                          'weather.example.org/temp(station, *)', 'data.example.org/price(*)',
                          'map(data.example.org/daily(*), v -> v * 2)', 'filter(data.example.org/daily(*), v -> v > 10 EUR)',
                          'scan(data.example.org/daily(*), sum)', 'window(data.example.org/daily(*), 3, mean)',
                          'data.example.org/price(*) * data.example.org/qty(*)', 'curve.example.net/df([1..3])',
                          'within(weather.example.org/temp(station, *), t, 30 min)'])

    def boolean(self, d=0):
        k = self.r.randrange(8)
        if d >= self.depth or k == 0:
            return self.pick(['flag', 'true', 'false', 'FALSE'])
        if k < 3:
            return '%s %s %s' % (self.num(d + 1), self.pick(['=', '<>', '<', '<=', '>', '>=']), self.num(d + 1))
        if k == 3:
            return '%s %s %s' % (self.boolean(d + 1), self.pick(['and', 'or', 'AND']), self.boolean(d + 1))
        if k == 4:
            return 'not ' + self.atom(self.boolean(d + 1))
        if k == 5:
            return self.pick(['and', 'or']) + '(%s, %s)' % (self.boolean(d + 1), self.boolean(d + 1))
        if k == 6:
            return 'is_holiday(%s, calendars.example.org/holiday("gb-eaw", *))' % self.date(d + 1)
        return '%s = %s' % (self.string(d + 1), self.string(d + 1))

    def string(self, d=0):
        k = self.r.randrange(8)
        if d >= self.depth or k == 0:
            return self.pick(STR_VARS + ['"abc"', '"Zürich"', '""', '"say ""hi"""'])
        if k == 1:
            return '%s & %s' % (self.string(d + 1), self.string(d + 1))
        if k == 2:
            return '%s(%s)' % (self.pick(['upper', 'lower', 'trim']), self.string(d + 1))
        if k == 3:
            return 'substr(%s, %s, %s)' % (self.string(d + 1), self.pick(['1', '2', '0']), self.pick(['3', '0', '10']))
        if k == 4:
            return 'text(%s)' % self.num(d + 1)
        if k == 5:
            return 'fixed(%s, %s)' % (self.num(d + 1), self.pick(['0', '2', '-1']))
        if k == 6:
            return 'concat(%s, %s)' % (self.string(d + 1), self.string(d + 1))
        return 'data.example.org/name(s)'

    def date(self, d=0):
        k = self.r.randrange(7)
        if d >= self.depth or k == 0:
            return self.pick(['d', 'd2', '2024-03-15', '2024-02-29'])
        if k == 1:
            return 'add_days(%s, %s)' % (self.date(d + 1), self.pick(['3', '-1', '30', 'n']))
        if k == 2:
            return 'add_months(%s, %s)' % (self.date(d + 1), self.pick(['1', '-1', '12']))
        if k == 3:
            return 'end_of_month(%s)' % self.date(d + 1)
        if k == 4:
            return 'date_of(%s)' % self.instant(d + 1)
        if k == 5:
            return '%s + %s' % (self.date(d + 1), self.pick(['1', '7 d', '30']))
        return 'date(2024, %s, %s)' % (self.pick(['1', '2', '13']), self.pick(['1', '29', '30']))

    def instant(self, d=0):
        k = self.r.randrange(4)
        if d >= self.depth or k == 0:
            return self.pick(['t', '2024-03-15T16:00:00.000000Z', 'asof()'])
        if k == 1:
            return '%s + %s' % (self.instant(d + 1), self.pick(['10 min', '1 h', '30', '1.5 s']))
        if k == 2:
            return 'instant(%s)' % self.date(d + 1)
        return '%s - 1 d' % self.instant(d + 1)

    def any(self):
        k = self.r.randrange(10)
        if k < 5:
            return self.num()
        return [self.boolean, self.string, self.date, self.instant, self.num][k - 5]()


MUTATIONS = ['(', ')', ',', '*', '[', '..', '"', '.', '^', '-', '+', '==', 'if', 'let', 'not', '@', 'EUR', 'now(',
             'this', '1.50', '1M', ' x', 'then', ':', '->', ';', '#', '2024-13-01', '\t', '\n']


def mutate(rng, s):
    for _ in range(rng.randrange(1, 3)):
        i = rng.randrange(len(s) + 1)
        k = rng.randrange(4)
        if k == 0 and s:
            s = s[:i] + s[i + 1:]
        elif k == 1:
            s = s[:i] + rng.choice(MUTATIONS) + s[i:]
        elif k == 2 and s:
            j = rng.randrange(len(s))
            s = s[:i] + s[j:j + rng.randrange(1, 6)] + s[i:]
        else:
            s = s[:i] + s[i:].swapcase()[:3] + s[i + 3:] if s else s
    return s


def formula_cases(n, seed, env):
    rng = random.Random(seed)
    for i in range(n):
        g = Gen(rng, rng.randrange(1, 5))
        src = g.any()
        if rng.random() < 0.35:
            src = mutate(rng, src)
        returns = rng.choice([{'type': 'number', 'unit': 'EUR'}, {'type': 'number', 'unit': '1'}, {'type': 'boolean'},
                              {'type': 'string'}, {'type': 'date'}, {'type': 'instant'}, {'type': 'number', 'unit': 'degC'}])
        e = json.loads(json.dumps(env))
        e['signature']['returns'] = returns
        yield {'id': 'gen/f%d' % i, 'kind': 'formula', 'description': 'generated', 'valid': True, 'input': src, 'env': e}


# Evaluation cases: bodies over a world of stored and computed functions (the union of the
# worlds of the formula-eval vectors), as typed templates.
T = {
    'EUR': ['a', 'b', '{EUR} + {EUR}', '{EUR} - {EUR}', '{EUR} * {N1}', '{EUR} / {N1}', '-{EUR}', 'abs({EUR})',
            'round({EUR}, {DIG})', 'round_up({EUR}, {DIG})', 'floor({EUR})', 'mod({EUR}, {EUR})',
            'if {B} then {EUR} else {EUR}', 'sum(data.example.org/daily(*))', 'at(data.example.org/daily(*), {D})',
            'otherwise(at(data.example.org/daily(*), {D}), 0)', 'last_before(data.example.org/daily(*), {D}, {DAYS})',
            'first_after(data.example.org/daily(*), {D})', 'nearest(data.example.org/daily(*), {D})',
            'interpolate(data.example.org/daily(*), {D})', 'interpolate(data.example.org/daily(*), {D}, "step")',
            'mean(data.example.org/daily([{D}..{D}]))', 'sum(data.example.org/daily([{D}..{D}]))',
            'data.example.org/daily(date: {D})', 'data.example.org/price(item: {S})',
            'otherwise(sum(data.example.org/price(*) * data.example.org/qty(*)), 0)',
            'max(window(data.example.org/daily(*), {SMALLN}, mean))', 'last(scan(data.example.org/daily(*), sum))',
            'min(scan(data.example.org/daily(*), max))', 'sum(map(data.example.org/daily(*), v -> v * {N1}))',
            'sum(map(data.example.org/daily(*), (v, date) -> if weekday(date) > 5 then 0 EUR else v))',
            'sum(filter(data.example.org/daily(*), v -> v > {EUR}))', 'median(data.example.org/daily(*))',
            'stdev(data.example.org/daily(*))', 'stdev_p(data.example.org/daily(*))', 'first(data.example.org/daily(*))',
            'asof(data.example.org/price(item: {S}), {T})', 'first(versions(data.example.org/price(item: {S}), {T}, asof()))',
            'last(versions(data.example.org/price(item: {S}), 2026-01-01T00:00:00.000000Z, {T}))',
            'let w = {EUR} in w * 2', 'mean({EUR}, {EUR}, 1)', 'sum({EUR}, {EUR})', 'max({EUR}, {EUR})', 'median({EUR}, {EUR}, {EUR})',
            'sum(data.example.org/price(*) * 2)', 'sum(data.example.org/price(*) - data.example.org/price(*))'],
    'N1': ['x', 'y', 'n', '0', '1', '2', '0.5', '3', '10', '1000', '0.0001', '{N1} + {N1}', '{N1} - {N1}', '{N1} * {N1}',
           '{N1} / {N1}', '({N1}) ^ {INT}', '({N1}) ^ 0.5', '({N1}) ^ -0.5', 'sqrt({N1})', 'exp({N1})', 'ln({N1})', 'sin({N1})',
           'cos({N1})', 'atan2({N1}, {N1})', 'pi()', 'math.example.net/fib(n: {SMALLN})', 'curve.example.net/df(tenor: {TENOR})',
           'sum(curve.example.net/df([1..{TENOR}]))', 'otherwise(sum(math.example.net/recip(*)), {N1})',
           'math.example.net/recip(n: {N1})', 'rates.example.org/rate("USD", "1M")', 'len({S})', 'find("m", {S})',
           'find("a", {S}, start: {SMALLN})', 'year({D})', 'weekday({D})', 'day(end_of_month({D}))', 'year_fraction({D}, {D}, "{CONV}")',
           '{EUR} / {EUR}', 'convert({N1}, "percent") / 100 percent', 'mod({N1}, {N1})', 'round({N1}, {DIG})', 'round_down({N1}, {DIG})',
           'ceiling({N1})', 'if {B} then {N1} else {N1}', 'count(data.example.org/daily(*)) / 1 count',
           'count(filter(weather.example.org/temp(*, *), v -> v > 10 degC)) / 1 count', 'sin({N1} deg)', 'convert(plat, "1")',
           'haversine(plat, 0, 48.85, 2.35) / 1 m', 'business_days({D}, {D}, calendar: calendars.example.org/holiday("gb-eaw", *)) / 1 d',
           'business_days({D}, {D}, weekend: "{WEEKEND}") / 1 d', '({D} - {D}) / 1 d', '(t - instant({D})) / 1 s',
           'rates.example.org/fx_rate("EUR", "USD", {D}) * 1 EUR / 1 USD', 'stdev({N1}, {N1})', 'mean(map(curve.example.net/df([1..3]), v -> v * v))'],
    'DEGC': ['t1', 'latest(weather.example.org/temp({S}, *))', 'last_before(weather.example.org/temp({S}, *), {T}, {MINS})',
             'interpolate(weather.example.org/temp({S}, *), {T})', 'nearest(weather.example.org/temp({S}, *), {T})',
             'nearest(weather.example.org/temp({S}, *), {T}, {MINS})', 'mean(last_before(weather.example.org/temp(*, *), {T}, {MINS}))',
             'weather.example.org/temp(station: {S}, t: {T})', 'mean(within(weather.example.org/temp({S}, *), {T}, {MINS}))',
             'latest(weather.example.org/temp({S}, *), {MINS})', 'first_after(weather.example.org/temp({S}, *), {T}, {MINS})',
             '{DEGC} + 1 K', '{DEGC} - (t1 - 5 degC)', 'convert(convert({DEGC}, "K"), "degC")', 'mean(weather.example.org/temp(*, *))',
             'max(weather.example.org/temp({S}, *))', 'latest(at(weather.example.org/temp(*, *), {S}, along: station))',
             'otherwise(latest(weather.example.org/temp({S}, *), {MINS}), t1)'],
    'B': ['flag', '{N1} > {N1}', '{EUR} = {EUR}', '{EUR} <> {EUR}', 'is_holiday({D}, calendars.example.org/holiday("gb-eaw", *))',
          'data.example.org/active(company: {S}) and flag', '{B} or {B}', '{B} and {B}', 'not {B}', '{S} = {S}', '{S} < {S}',
          'data.example.org/doc(company: {S}) = data.example.org/doc(company: {S})', '{D} < {D}', '{T} >= {T}', '{DEGC} > {DEGC}'],
    'S': ['s', '"abc"', '"Zürich"', 'upper({S})', 'lower({S})', 'trim({S})', '{S} & {S}', 'concat({S}, " ", {S})',
          'substr({S}, {SMALLN}, {SMALLN})', 'text({N1})', 'text({D})', 'text({B})', 'text({T})', 'fixed({N1}, {DIG})', 'fixed({EUR}, 2)',
          'data.example.org/name(company: {S})', 'if {B} then {S} else {S}', 'min(data.example.org/name(*))'],
    'D': ['d', 'd2', '2026-03-03', 'add_days({D}, {INT})', 'add_days({D}, {INT}, calendar: calendars.example.org/holiday("gb-eaw", *))',
          'add_days({D}, {INT}, weekend: "{WEEKEND}")', 'add_months({D}, {INT})',
          'add_months({D}, {INT}, calendar: calendars.example.org/holiday("gb-eaw", *), roll: "{ROLL}")', 'end_of_month({D})',
          'date_of({T})', 'date(2026, {SMALLN}, {SMALLN})', 'data.example.org/incorporated(company: {S})', '{D} + {INT}', '{D} - 7 d',
          'first(calendars.example.org/holiday("gb-eaw", *))', 'max(d, d2)'],
    'T': ['t', 'asof()', '{T} + {MINS}', '{T} - 1 h', 'instant({D})', 't + 0.5 s', 'max(t, asof())'],
}
FILL = {'DIG': ['0', '1', '2', '-1', '3'], 'INT': ['0', '1', '2', '3', '-1', '-3', '10', '12'], 'SMALLN': ['0', '1', '2', '3', '5', '7'],
        'TENOR': ['1', '2', '3', '4', '10', '11'], 'DAYS': ['1 d', '2 d', '7 d', '30 d'], 'MINS': ['1 min', '5 min', '10 min', '15 min', '1 h', '30 s'],
        'CONV': ['act/360', 'act/365f', 'act/act_isda', '30/360', '30e/360', '30/360_us'], 'WEEKEND': ['sat-sun', 'fri-sat', 'sun', 'none'],
        'ROLL': ['following', 'modified_following', 'preceding']}
RETURNS = {'EUR': {'type': 'number', 'unit': 'EUR'}, 'N1': {'type': 'number', 'unit': '1'}, 'DEGC': {'type': 'number', 'unit': 'degC'},
           'B': {'type': 'boolean'}, 'S': {'type': 'string'}, 'D': {'type': 'date'}, 'T': {'type': 'instant'}}
E_ARGS = {'a': ('number', 'EUR'), 'b': ('number', 'EUR'), 'x': ('number', '1'), 'y': ('number', '1'), 'n': ('number', '1'),
          'd': ('date', None), 'd2': ('date', None), 't': ('instant', None), 's': ('string', None), 'flag': ('boolean', None),
          't1': ('number', 'degC'), 'plat': ('number', 'deg')}
VALUES = {'a': ['-4.2', '10', '0.5', '1234.5678', '0', '12'], 'b': ['3', '-1', '0', '11.5'], 'x': ['0', '1', '-1', '2', '0.5', '3', '1000', '0.0001'],
          'y': ['0', '1', '2', '-0.25', '7'], 'n': ['0', '1', '2', '5', '10', '20', '-1', '3'],
          'd': ['2026-03-01', '2026-03-03', '2026-03-05', '2026-03-10', '2024-01-31', '2027-01-01', '2026-04-01', '2026-04-30'],
          'd2': ['2026-03-04', '2026-04-10', '2026-03-01', '2025-12-31'],
          't': ['2026-03-01T10:00:00.000000Z', '2026-03-01T10:05:00.000000Z', '2026-03-01T10:03:30.000000Z', '2026-03-01T10:08:00.000000Z',
                '2026-03-01T11:00:00.000000Z', '2026-03-15T00:00:00.000000Z', '2026-07-01T00:00:00.000000Z'],
          's': ['kew', 'ely', 'bolt', 'nut', 'acme', 'Zürich', '  a  b ', 'mama'], 'flag': [True, False], 't1': ['20', '-3.5', '0'],
          'plat': ['51.5', '0', '-33.9']}
ASOFS = ['2026-06-01T00:00:00.000000Z', '2026-03-01T10:09:00.000000Z', '2026-03-01T11:00:00.000000Z', '2026-03-01T00:00:00.000000Z',
         '2026-02-15T00:00:00.000000Z', '2026-03-10T12:00:00.000000Z']


def template(rng, kind, depth):
    choices = T[kind]
    if depth <= 0:
        simple = [c for c in choices if '{' not in c]
        choices = simple or choices
    t = rng.choice(choices)
    out = ''
    i = 0
    while i < len(t):
        if t[i] == '{':
            j = t.index('}', i)
            hole = t[i + 1:j]
            if hole in T:
                sub = template(rng, hole, depth - 1)
                out += sub if all(ch.isalnum() or ch in '_."' for ch in sub) else '(' + sub + ')'
            elif hole in FILL:
                out += rng.choice(FILL[hole])
            else:
                out += t[i:j + 1]
            i = j + 1
        else:
            out += t[i]
            i += 1
    return out


def world_union(vector_dir):
    import glob
    fs, rs = {}, {}
    for f in glob.glob(vector_dir + '/formula-eval/*.json') + glob.glob(vector_dir + '/invalid/formula-eval-*.json') + glob.glob(vector_dir + '/excel/*.json'):
        v = json.load(open(f))
        for n, e in v['world']['functions'].items():
            if n in ('calc.example.net/e', 'sheet.example.net/case'):
                continue
            fs[n] = e
        for r in v['world']['receipts']:
            rs[json.dumps(r, sort_keys=True)] = r
    return fs, list(rs.values())


def e_signature(returns):
    args = {}
    for k, (t, u) in E_ARGS.items():
        args[k] = {'type': t, 'valid': t in ('date', 'instant')}
        if u:
            args[k]['unit'] = u
    return {'args': args, 'order': sorted(args), 'returns': returns}


def eval_sources(n, seed, vector_dir):
    """Step 1: formula vectors for random bodies of calc.example.net/e over the union world."""
    rng = random.Random(seed)
    fs, _ = world_union(vector_dir)
    for i in range(n):
        kind = rng.choice(list(T))
        src = template(rng, kind, rng.randrange(1, 4))
        env = {'name': 'calc.example.net/e', 'signature': e_signature(RETURNS[kind]),
               'functions': {k: e['descriptor'] for k, e in fs.items()}, 'aliases': {}}
        yield {'id': 'gen/s%d' % i, 'kind': 'formula', 'description': 'generated', 'valid': True, 'input': src, 'env': env}


def eval_cases(sources, defined, seed, vector_dir):
    """Step 2: formula-eval vectors from the bodies that define."""
    rng = random.Random(seed)
    fs, receipts = world_union(vector_dir)
    for i, (src, out) in enumerate(zip(sources, defined)):
        if 'body' not in out:
            continue
        sig = src['env']['signature']
        desc = {'code': {'body': out['body'], 'lang': 'formula'}, 'inputs': out['inputs'], 'signature': sig}
        world = {'functions': dict(fs), 'receipts': receipts}
        world['functions']['calc.example.net/e'] = {'bound': '2026-01-01T00:00:00.000000Z', 'descriptor': desc}
        args = {}
        for k, (t, _) in E_ARGS.items():
            args[k] = {'type': t, 'value': rng.choice(VALUES[k])}
        v = {'id': 'gen/e%d' % i, 'kind': 'formula-eval', 'description': 'generated', 'valid': True, 'world': world,
             'cell': {'fn': 'calc.example.net/e', 'args': args}, 'asof': rng.choice(ASOFS)}
        if rng.random() < 0.1:
            v['fuel'] = rng.randrange(1, 200)
        yield v


def main():
    kind, n, seed = sys.argv[1], int(sys.argv[2]), int(sys.argv[3])
    if kind == 'formula':
        env = json.load(open(sys.argv[4]))
        for c in formula_cases(n, seed, env):
            print(json.dumps(c, ensure_ascii=False))
    elif kind == 'eval-sources':
        for c in eval_sources(n, seed, sys.argv[4]):
            print(json.dumps(c, ensure_ascii=False))
    elif kind == 'eval':
        sources = [json.loads(l) for l in open(sys.argv[4])]
        defined = [json.loads(l) for l in open(sys.argv[5])]
        for c in eval_cases(sources, defined, seed, sys.argv[6]):
            print(json.dumps(c, ensure_ascii=False))
    else:
        raise SystemExit('unknown kind ' + kind)


if __name__ == '__main__':
    main()
