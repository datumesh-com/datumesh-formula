#!/usr/bin/env python3
"""Run vectors (JSON lines on stdin) through the reference tool tools/formula.py of a
datumesh-spec checkout, as a black box, and print its outputs as JSON lines.

    python3 tools/reference_runner.py /path/to/datumesh-spec < cases.jsonl > ref.jsonl

Needs Python >= 3.13 and PyNaCl (the reference's own requirements).
"""
import json
import sys

sys.path.insert(0, sys.argv[1] + '/tools')
sys.setrecursionlimit(100000)
import formula  # noqa: E402

for line in sys.stdin:
    v = json.loads(line)
    try:
        r = formula.run_vector(v)
    except RecursionError:
        r = {'error': 'RecursionError'}
    except Exception as e:  # a crash of the reference: reported, not hidden
        r = {'error': 'EXCEPTION ' + type(e).__name__}
    r.pop('detail', None)
    print(json.dumps(r, ensure_ascii=False, sort_keys=True, separators=(',', ':')))
