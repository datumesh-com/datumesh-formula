#!/usr/bin/env python3
"""Compare two JSON-lines result files case by case and print the differences.

    python3 tools/compare.py cases.jsonl ours.jsonl theirs.jsonl [max_shown]
"""
import json
import sys

cases = [json.loads(l) for l in open(sys.argv[1])]
ours = [json.loads(l) for l in open(sys.argv[2])]
theirs = [json.loads(l) for l in open(sys.argv[3])]
shown = int(sys.argv[4]) if len(sys.argv) > 4 else 50
assert len(cases) == len(ours) == len(theirs), (len(cases), len(ours), len(theirs))
diff = 0
crashes = 0
for c, a, b in zip(cases, ours, theirs):
    b.pop('detail', None)
    if b.get('error', '').startswith(('EXCEPTION', 'RecursionError')):
        crashes += 1
        continue
    if a != b:
        diff += 1
        if diff <= shown:
            src = c.get('input') or c.get('world', {}).get('functions', {}).get(c.get('cell', {}).get('fn'), {}).get('descriptor', {}).get('code', {}).get('body')
            print('CASE', json.dumps(src, ensure_ascii=False)[:200])
            keys = sorted(set(a) | set(b))
            for k in keys:
                if a.get(k) != b.get(k):
                    print('   %s: ours %s | ref %s' % (k, json.dumps(a.get(k), ensure_ascii=False)[:150], json.dumps(b.get(k), ensure_ascii=False)[:150]))
print('cases %d, differences %d, reference crashes skipped %d' % (len(cases), diff, crashes))
