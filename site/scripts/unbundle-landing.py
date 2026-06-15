#!/usr/bin/env python3
"""Un-bundle a Claude Design "standalone HTML" export into a clean, static,
SEO-friendly landing page wired for the Astro site at base path /escape-the-valley.

The export is a self-extracting bundle:
  - <script type="__bundler/manifest"> : JSON map UUID -> {mime, compressed, data(base64)}
  - <script type="__bundler/template"> : JSON-string of the REAL page HTML, with
    asset references as UUIDs, Claude-Design wrappers (<x-dc>/<helmet>), template
    vars ({{ ember }} etc.), <sc-if> conditionals, and a <script data-dc-script>
    React component that streams the fireside narration token-by-token.
  - a runtime <script> + #__bundler_thumbnail + #__bundler_loading "Unpacking..."
    scaffolding that assembles the page on load.

What this script produces (no JS required to see content, no Unpacking flash,
no 2.9MB payload):
  - site/public/index.html  : the clean static landing page
  - site/public/fonts/*.woff2 : the 45 self-hosted Google font subsets (decoded)
  - site/public/ev-logo.png : the footer logo (1.44MB PNG written as a real asset,
                              NOT inlined -- inlining would re-bloat the HTML)

Run:  python site/scripts/unbundle-landing.py
"""
from __future__ import annotations

import base64
import gzip
import html as html_mod
import json
import re
from pathlib import Path

# ---------------------------------------------------------------------------
# Paths / config
# ---------------------------------------------------------------------------
EXPORT = Path(r"C:/Users/mikey/Downloads/Escape the Valley.html")
SITE = Path(__file__).resolve().parents[1]          # site/
PUBLIC = SITE / "public"
FONTS_DIR = PUBLIC / "fonts"
BASE = "/escape-the-valley"                          # astro.config.mjs base
OUT_HTML = PUBLIC / "index.html"
LOGO_NAME = "ev-logo.png"

# Claude-Design template var defaults (from data-props of the dc-script block)
TVARS = {"ember": "#d97706", "cold": "#6fa6a0", "grain": "true"}

# ---------------------------------------------------------------------------
# Load the bundle
# ---------------------------------------------------------------------------
raw = EXPORT.read_text(encoding="utf-8")

manifest = json.loads(
    re.search(r'<script type="__bundler/manifest">(.*?)</script>', raw, re.DOTALL).group(1)
)
template = json.loads(
    re.search(r'<script type="__bundler/template">(.*?)</script>', raw, re.DOTALL).group(1).strip()
)


def asset_bytes(uuid: str) -> bytes:
    """Decode a manifest asset to raw bytes (gunzip if flagged)."""
    a = manifest[uuid]
    data = base64.b64decode(a["data"])
    if a.get("compressed"):
        data = gzip.decompress(data)
    return data


# ---------------------------------------------------------------------------
# 1. Self-host the fonts: parse @font-face blocks -> deterministic filenames
# ---------------------------------------------------------------------------
def slug(s: str) -> str:
    return re.sub(r"[^a-z0-9]+", "-", s.lower()).strip("-")


FONTS_DIR.mkdir(parents=True, exist_ok=True)
# wipe stale generated woff2 so the dir is deterministic
for f in FONTS_DIR.glob("*.woff2"):
    f.unlink()

uuid_to_fontpath: dict[str, str] = {}
font_count = 0
for blk in re.finditer(
    r"(?:/\*\s*([^*]+?)\s*\*/\s*)?@font-face\s*\{(.*?)\}", template, re.DOTALL
):
    subset = blk.group(1) or "default"
    body = blk.group(2)
    fam = re.search(r"font-family:\s*'([^']+)'", body)
    wt = re.search(r"font-weight:\s*(\d+)", body)
    style = re.search(r"font-style:\s*(\w+)", body)
    uuid = re.search(r'url\("([0-9a-f-]{30,})"\)', body)
    if not (fam and uuid):
        continue
    fname = f"{slug(fam.group(1))}-{wt.group(1) if wt else '400'}-{style.group(1) if style else 'normal'}-{slug(subset)}.woff2"
    (FONTS_DIR / fname).write_bytes(asset_bytes(uuid.group(1)))
    uuid_to_fontpath[uuid.group(1)] = f"{BASE}/fonts/{fname}"
    font_count += 1

# ---------------------------------------------------------------------------
# 2. Write the logo as a real asset (NOT inlined -- it is 1.44MB)
# ---------------------------------------------------------------------------
logo_uuid = next(u for u, a in manifest.items() if a["mime"] == "image/png")
(PUBLIC / LOGO_NAME).write_bytes(asset_bytes(logo_uuid))
logo_path = f"{BASE}/{LOGO_NAME}"

# ---------------------------------------------------------------------------
# 3. Reconstruct the page HTML from the template
# ---------------------------------------------------------------------------
page = template

# 3a. Rewrite asset URLs: fonts -> self-hosted path, logo -> public asset
for uuid, fp in uuid_to_fontpath.items():
    page = page.replace(f'url("{uuid}")', f'url("{fp}")')
page = page.replace(f'src="{logo_uuid}"', f'src="{logo_path}"')

# 3b. Drop the bundler-runtime <script src="<js-uuid>"> in <head>
js_uuid = next(u for u, a in manifest.items() if a["mime"] == "text/javascript")
page = re.sub(rf'<script src="{js_uuid}">\s*</script>\s*', "", page)

# 3c. Resolve <sc-if value="{{ grain }}" ...> -- grain default is true, so KEEP
#     the wrapped content and unwrap the tag.
page = re.sub(r'<sc-if[^>]*>', "", page)
page = page.replace("</sc-if>", "")

# 3d. Static-render the fireside narration that the dc-script streamed.
#     Tokens (and their "cold" flag) are lifted from the dc-script component so
#     the page shows the full sentence with NO JavaScript. A small progressive
#     -enhancement script (added below) replays the streaming reveal when JS is on.
TOKENS = [
    ("The", False), ("fog", True), ("came", False), ("up", False), ("off", False),
    ("the", False), ("river", False), ("before", False), ("dusk", False), ("and", False),
    ("did", False), ("not", False), ("lift.", False), ("Wren", False), ("swears", False),
    ("she", False), ("heard", False), ("a", False), ("second", True), ("axe", True),
    ("in", False), ("the", False), ("treeline,", False), ("keeping", False), ("time", False),
    ("with", False), ("Samuel's.", False), ("He", False), ("says", False), ("it", False),
    ("was", False), ("the", False), ("echo.", True),
]
EMBER, COLD = TVARS["ember"], TVARS["cold"]
spans = []
for i, (t, cold) in enumerate(TOKENS):
    color = COLD if cold else "#d8cbb2"
    spans.append(
        f'<span class="ev-tok" data-i="{i}" style="color: {color};">{html_mod.escape(t)} </span>'
    )
cursor = f'<span class="ev-cursor" style="color: {EMBER}; font-weight: 600;">▋</span>'
narration_html = f'<span id="ev-narration">{"".join(spans)}{cursor}</span>'
page = re.sub(r"\{\{\s*narrationEl\s*\}\}", narration_html, page)

# 3e. Substitute the remaining template vars ({{ ember }}, {{ cold }}, {{ grain }}).
for k, v in TVARS.items():
    page = re.sub(r"\{\{\s*" + re.escape(k) + r"\s*\}\}", v, page)

# 3f. Remove the dc-script component block (its job is now baked into 3d + the PE script).
page = re.sub(
    r'<script type="text/x-dc"[^>]*>.*?</script>', "", page, flags=re.DOTALL
)

# 3g. Unwrap Claude-Design structural wrappers <x-dc>/<helmet> (keep their children).
for tag in ("x-dc", "helmet"):
    page = re.sub(rf"</?{tag}[^>]*>", "", page)

# 3h. Fix the corrupted middot separator (mojibake) -> proper middot.
page = page.replace("LEDGER TRAIL � v1.1", "LEDGER TRAIL · v1.1")
page = page.replace("�", "·")  # any other stray replacement chars

# 3i. Drop the Google Fonts <link rel="preconnect"> lines -- we self-host now, so
#     they would be a pointless connection to Google (and undercut "GDPR-clean").
page = re.sub(r'\s*<link rel="preconnect" href="https://fonts\.[^"]+"[^>]*>', "", page)

# ---------------------------------------------------------------------------
# 4. Extract the reconstructed <head> contents and <body> contents, then rebuild
#    a clean document with a real <title>, meta description, and the fonts.
# ---------------------------------------------------------------------------
# The original head only had charset/viewport (+ the runtime script we removed).
# All the font/page <style> lives inside what was <helmet> (now unwrapped, in body).
body_inner = re.search(r"<body>(.*)</body>", page, re.DOTALL).group(1).strip()

# Progressive-enhancement script: replay the streaming narration reveal + honor
# reduced-motion. Content is already fully visible without it.
PE_SCRIPT = """
<script>
(function () {
  var wrap = document.getElementById('ev-narration');
  if (!wrap) return;
  var toks = Array.prototype.slice.call(wrap.querySelectorAll('.ev-tok'));
  var reduce = window.matchMedia && window.matchMedia('(prefers-reduced-motion: reduce)').matches;
  if (reduce || !toks.length) return;  // leave fully revealed
  toks.forEach(function (s) { s.style.opacity = '0'; });
  var i = 0;
  var iv = setInterval(function () {
    if (i >= toks.length) { clearInterval(iv); return; }
    toks[i].style.transition = 'opacity .18s ease';
    toks[i].style.opacity = '1';
    i++;
  }, 95);
})();
</script>
"""

DESCRIPTION = (
    "Escape the Valley: Ledger Trail — an Oregon Trail-style survival RPG "
    "where every supply is reconciled on an on-ledger proof harness. LLM-driven "
    "Game Master, terminal UI, multiplayer parcel trading over XRPL. "
    "pip install escape-the-valley."
)

doc = f"""<!DOCTYPE html>
<html lang="en">
<head>
<meta charset="utf-8">
<meta name="viewport" content="width=device-width, initial-scale=1">
<title>Escape the Valley: Ledger Trail</title>
<meta name="description" content="{html_mod.escape(DESCRIPTION)}">
<meta property="og:title" content="Escape the Valley: Ledger Trail">
<meta property="og:description" content="{html_mod.escape(DESCRIPTION)}">
<meta property="og:type" content="website">
<meta property="og:image" content="{logo_path}">
<meta name="twitter:card" content="summary_large_image">
</head>
<body>
{body_inner}
{PE_SCRIPT.strip()}
</body>
</html>
"""

OUT_HTML.write_text(doc, encoding="utf-8")

# ---------------------------------------------------------------------------
# Report
# ---------------------------------------------------------------------------
total_fonts_bytes = sum(f.stat().st_size for f in FONTS_DIR.glob("*.woff2"))
print(f"fonts self-hosted : {font_count} woff2 -> {FONTS_DIR}  ({total_fonts_bytes // 1024} KB total)")
print(f"logo asset        : {PUBLIC / LOGO_NAME}  ({(PUBLIC / LOGO_NAME).stat().st_size // 1024} KB)")
print(f"landing page      : {OUT_HTML}  ({OUT_HTML.stat().st_size // 1024} KB)")
print(f"remaining UUID refs: {len(re.findall(r'[0-9a-f]{8}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{12}', doc))} (should be 0)")
