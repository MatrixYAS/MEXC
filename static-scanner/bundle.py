#!/usr/bin/env python3
"""Inline pb.js, engine.js, scanner.js into index.html as one self-contained file."""
import re
BASE = "/home/ubuntu/static-scanner"
IMPORT_RE = re.compile(r'^import[^;]*"\./[a-z.]+";\s*$', re.M)
parts = []
for name in ("pb.js", "engine.js", "scanner.js"):
    s = open(f"{BASE}/{name}", encoding="utf-8").read()
    s = IMPORT_RE.sub("", s)
    s = re.sub(r'^export (function|class|const|let|async function)', r"\1", s, flags=re.M)
    parts.append(f"// ===== {name} =====\n" + s)

html = open(f"{BASE}/index.html", encoding="utf-8").read()
m = re.search(r'<script type="module">(.*?)</script>', html, re.S)
assert m, "no module script in index.html"
ui = IMPORT_RE.sub("", m.group(1))

out = html[:m.start()]
out += "<script type=\"module\">\n" + "\n".join(parts) + "\n// ===== ui =====\n" + ui + "\n</script>\n</body>\n</html>"
open(f"{BASE}/index.bundled.html", "w", encoding="utf-8").write(out)
print("bundled:", len(out), "bytes")
