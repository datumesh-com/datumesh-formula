# Generate Unicode 15.1 full case mappings (str.upper/str.lower per code point) and the
# Cased / Case_Ignorable properties, from Python 3.13's unicodedata 15.1, for datumesh-formula.
import sys, unicodedata
assert unicodedata.unidata_version == '15.1.0', unicodedata.unidata_version
SIGMA='Σ'
up=[];lo=[];cased=[];ign=[]
for cp in range(0x110000):
    if 0xD800 <= cp <= 0xDFFF: continue
    c=chr(cp)
    if unicodedata.category(c)=='Cn': continue
    u=c.upper(); l=c.lower()
    if u!=c: up.append((cp,u))
    if l!=c and cp!=0x03A3: lo.append((cp,l))
    p1 = (c+SIGMA).lower()[-1]=='ς'
    p2 = ('A'+c+SIGMA).lower()[-1]=='ς'
    is_ign = (not p1) and p2
    is_cased = p1
    if is_ign: ign.append(cp)
    if is_cased: cased.append(cp)
def ranges(cps):
    out=[]
    for cp in cps:
        if out and out[-1][1]==cp-1: out[-1][1]=cp
        else: out.append([cp,cp])
    return out
def esc(s): return ''.join('\\u{%x}'%ord(ch) for ch in s)
w=sys.stdout.write
w('//! Unicode 15.1 case data, generated from Python 3.13 (unicodedata 15.1.0) by\n//! tools/gen_casing.py: full case mappings and the Cased and Case_Ignorable properties.\n\n')
w('/// Full uppercase mappings that differ from the code point, ascending.\npub(crate) static UPPER: &[(u32, &str)] = &[\n')
for cp,s in up: w('    (0x%x, "%s"),\n'%(cp,esc(s)))
w('];\n\n/// Full lowercase mappings that differ from the code point (U+03A3 handled by Final_Sigma), ascending.\npub(crate) static LOWER: &[(u32, &str)] = &[\n')
for cp,s in lo: w('    (0x%x, "%s"),\n'%(cp,esc(s)))
w('];\n\n/// Cased (that are not Case_Ignorable), as inclusive ranges.\npub(crate) static CASED: &[(u32, u32)] = &[\n')
for a,b in ranges(cased): w('    (0x%x, 0x%x),\n'%(a,b))
w('];\n\n/// Case_Ignorable, as inclusive ranges.\npub(crate) static CASE_IGNORABLE: &[(u32, u32)] = &[\n')
for a,b in ranges(ign): w('    (0x%x, 0x%x),\n'%(a,b))
w('];\n')
