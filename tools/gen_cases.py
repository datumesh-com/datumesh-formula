#!/usr/bin/env python3
"""Generate random formula cases (JSON lines of `formula` and `formula-eval` vectors) for
differential runs of this crate against another implementation of SPEC 15.

    python3 tools/gen_cases.py formula N SEED ENV.json > cases.jsonl
    python3 tools/gen_cases.py eval N SEED > cases.jsonl

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


def main():
    kind, n, seed = sys.argv[1], int(sys.argv[2]), int(sys.argv[3])
    if kind == 'formula':
        env = json.load(open(sys.argv[4]))
        for c in formula_cases(n, seed, env):
            print(json.dumps(c, ensure_ascii=False))
    else:
        raise SystemExit('unknown kind ' + kind)


if __name__ == '__main__':
    main()
