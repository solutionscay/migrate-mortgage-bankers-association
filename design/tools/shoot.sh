#!/usr/bin/env bash
# 1440x900 headless Chrome screenshot of a design page into design/screenshots/.
#
#   design/tools/shoot.sh <page.html> "<query>" <output-name>
#   design/tools/shoot.sh registro.html "?role=staff&at=pagos" registro-staff
#   design/tools/shoot.sh components.html "" components-top
#
# Query hooks (assets/app.js): role=admin|staff, scroll=<px>, at=<element id>, open=<dialog id>.
# Never drives the visible screen. A fresh browser profile per shot avoids stale CSS.
# Set CHROME=/path/to/chromium to use another browser.
#
#   design/tools/shoot.sh --compare a.png b.png
# reports whether the header (rows 0-147) and footer (last 28 rows) of two shots match.
set -euo pipefail
DESIGN="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"

if [[ "${1:-}" == "--compare" ]]; then
  python3 - "$2" "$3" <<'PY'
import sys
from PIL import Image, ImageChops
a, b = (Image.open(p).convert("RGB") for p in sys.argv[1:3])
W, H = a.size
ok = True
for label, box in (("header", (0, 0, W, 148)), ("footer", (0, H - 28, W, H))):
    d = ImageChops.difference(a.crop(box), b.crop(box)).getbbox()
    ok &= d is None
    print(f"{label}: {'IDENTICAL' if d is None else 'DIFF at ' + str(d)}")
sys.exit(0 if ok else 1)
PY
  exit $?
fi

PAGE="${1:?page, e.g. registro.html}"
QUERY="${2:-}"
NAME="${3:?output name without .png}"
CHROME="${CHROME:-$(command -v chromium || command -v chromium-browser || command -v google-chrome)}"
OUT="$DESIGN/screenshots/$NAME.png"
PROFILE="$(mktemp -d)"
trap '[[ -n "$PROFILE" && -d "$PROFILE" ]] && rm -r "$PROFILE"' EXIT

"$CHROME" --headless=new --disable-gpu --no-first-run --user-data-dir="$PROFILE" \
  --force-device-scale-factor=1 --window-size=1440,900 --virtual-time-budget=3000 \
  --screenshot="$OUT" "file://$DESIGN/$PAGE$QUERY" 2>/dev/null
echo "$OUT"
