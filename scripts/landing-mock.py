#!/usr/bin/env python3
"""DESIGN-58: the landing page as one document a ticket body can hold — the
mock (DESIGN-49), shown with `view: mockup` at 390 and 1440. style.css with
its fonts and grain inlined, every media reference inlined as a data URI
(the 1200 WebP of each still, the animated WebP of each clip; the AVIF
sources, the posters and the 2400 candidates dropped), main.js inlined, the
guide's links pointed at the live guide.

  scripts/landing-mock.py [LANDING_DIR] OUT.html      (LANDING_DIR: landing/)

Then the mock doc's body is replaced with it — too large for an argument,
so through deploy/menger-call.mjs on stdin:

  python3 -c 'import json,sys; print(json.dumps({"id":"DESIGN-49","body":open(sys.argv[1]).read()}))' OUT.html \
    | node deploy/menger-call.mjs --url ws://127.0.0.1:4455 tickets.amend
"""
import base64, pathlib, re, sys

args = sys.argv[1:]
if not args or len(args) > 2:
    sys.exit(__doc__)
LANDING = pathlib.Path(args[0] if len(args) == 2 else pathlib.Path(__file__).resolve().parent.parent / "landing")
OUT = pathlib.Path(args[-1])

def data(path, mime):
    return f"data:{mime};base64," + base64.b64encode((LANDING / path).read_bytes()).decode()

css = (LANDING / "style.css").read_text()
css = css.replace("url(fonts/geist-latin.woff2)", f"url({data('fonts/geist-latin.woff2', 'font/woff2')})")
css = css.replace("url(fonts/geist-mono-latin.woff2)", f"url({data('fonts/geist-mono-latin.woff2', 'font/woff2')})")
css = css.replace("url(grain.svg)", f"url({data('grain.svg', 'image/svg+xml')})")
assert "url(fonts/" not in css and "url(grain" not in css

html = (LANDING / "index.html").read_text()
# the head: the stylesheet and the script inlined, preloads and the icon dropped
html = re.sub(r'<link rel="preload"[^>]*>\n', "", html)
html = re.sub(r'<link rel="icon"[^>]*>\n', "", html)
html = html.replace('<link rel="stylesheet" href="style.css">', "<style>\n" + css + "\n</style>")
js = (LANDING / "main.js").read_text()
# the script was deferred: inlined, it goes at the end of the body, after everything it looks up
html = html.replace('<script src="main.js" defer></script>\n', "")
html = html.replace("</body>", "<script>\n" + js + "\n</script>\n</body>")
assert html.count("<script>") == 1 and "<script src" not in html
html = html.replace('<meta name="color-scheme" content="light dark">',
                    '<meta name="color-scheme" content="light dark">\n<meta name="menger-view" content="mockup">\n<meta name="menger-widths" content="390,1440">')
# pictures: drop every <source> (AVIF, reduced-motion posters), the srcset/sizes, and inline the src
html = re.sub(r'<source [^>]*>', "", html)
html = re.sub(r' srcset="[^"]*"', "", html)
html = re.sub(r' sizes="[^"]*"', "", html)
def inline_src(m):
    path = m.group(1)
    mime = "image/webp" if path.endswith(".webp") else "image/png"
    return f' src="{data(path, mime)}"'
html = re.sub(r' src="(media/[^"]+)"', inline_src, html)
assert 'src="media/' not in html
# links the frame cannot follow: the guide, relative — pointed at the live guide
html = html.replace('href="guide/', 'href="https://menger.sh/guide/')
OUT.write_text(html)
print(OUT, len(html.encode()), "bytes")
