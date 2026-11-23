#!/usr/bin/env bash
# Record the desk's report exchanges from a Minos deployment (#278).
#
#   MINOS_BASE_URL=https://…/api/v1 \
#   MINOS_IDENTIFIER=… MINOS_PASSWORD=… scripts/record-desk-report.sh
#
# **Only refusals are recorded.** Every exchange this script makes writes nothing: a missing field, a hull with
# no commander appointed, a time older than 30 days, a hull that does not exist, and a body the route cannot
# read. The accepted answer is a **201 that stores a fix for a real vessel**, so it is deliberately not here:
# a human decides when one goes into a shared deployment, and the fixture is replaced by hand with the bytes
# that came back.
#
# The credential arrives in the **environment**, never as an argument (an argument is in the shell's history and
# in `ps`), and the token from `/auth/login` is never printed, never written to a fixture and never logged. What
# the files hold is the request body and the answer body, which is what the desk's tests read.
#
# The route is newer than the pinned contract (`docs/spec/minos-api.yaml` has `/live-feed/fixes` only), so these
# files are the wire's record of its shape until the spec is re-vendored from a Minos commit that carries it.
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$ROOT"

: "${MINOS_BASE_URL:?set MINOS_BASE_URL, e.g. https://api.example/api/v1}"
: "${MINOS_IDENTIFIER:?set MINOS_IDENTIFIER}"
: "${MINOS_PASSWORD:?set MINOS_PASSWORD}"

OUT="fixtures/desk/report"
mkdir -p "$OUT"

WORK="$(mktemp -d)"
trap 'rm -rf "$WORK"' EXIT
chmod 700 "$WORK"

python3 - "$WORK/login.json" <<'PY'
import json, os, sys
json.dump({"identifier": os.environ["MINOS_IDENTIFIER"], "password": os.environ["MINOS_PASSWORD"]},
          open(sys.argv[1], "w"))
PY
chmod 600 "$WORK/login.json"

login="$WORK/login.json"
token="$(curl -sS -X POST "$MINOS_BASE_URL/auth/login" -H 'content-type: application/json' \
  --data-binary "@$login" --max-time 30 | python3 -c 'import json,sys; print(json.load(sys.stdin).get("data",{}).get("access_token",""))')"
rm -f "$login"
if [ -z "$token" ]; then
  echo "record-desk-report: /auth/login returned no access token" >&2
  exit 1
fi

# The instant every "too old" exchange is measured from: 40 days back from now, in UTC.
old_at="$(python3 -c 'import datetime; print((datetime.datetime.now(datetime.timezone.utc)-datetime.timedelta(days=40)).strftime("%Y-%m-%dT%H:%M:%S.000Z"))')"

# post <name> <json-body>
post() {
  local name="$1" body="$2" answer status
  answer="$WORK/answer.json"
  printf '%s' "$body" > "$WORK/request.json"
  status="$(curl -sS -o "$answer" -w '%{http_code}' -X POST "$MINOS_BASE_URL/live-feed/fixes/relayed" \
    -H "authorization: Bearer $token" -H 'content-type: application/json' \
    --data-binary "@$WORK/request.json" --max-time 30)"
  # The base URL is deliberately **not** written: a deployment host is exactly what INV-08 keeps out of the
  # tree, and a fixture does not need it to be replayed.
  python3 - "$OUT/$name.json" "$status" "$WORK/request.json" "$answer" <<'PY'
import json, sys, datetime
path, status, req, ans = sys.argv[1:5]
def load(p):
    with open(p) as f:
        return json.load(f)
doc = {
    "recorded": datetime.date.today().isoformat(),
    "against": "the development deployment",
    "request": {"method": "POST", "path": "/live-feed/fixes/relayed", "body": load(req)},
    "response": {"status": int(status), "body": load(ans)},
}
with open(path, "w") as f:
    json.dump(doc, f, indent=2, sort_keys=False)
    f.write("\n")
print(f"  {path} ({status})")
PY
}

echo "recording refusals (nothing is written by any of them):"

post required-fields '{}'

post no-commander "$(python3 -c 'import json,sys; print(json.dumps({"id_unit": 1, "latitude": -6.1, "longitude": 106.87, "recorded_at": sys.argv[1]}))' "$old_at")"

# The one hull on the development deployment with a commander appointed, from `/live-feed/positions`; the age
# rule is checked after the commander, so a hull with no commander can never reach this refusal.
post age "$(python3 -c 'import json,sys; print(json.dumps({"id_unit": 8, "latitude": -6.1, "longitude": 106.87, "recorded_at": sys.argv[1]}))' "$old_at")"

post no-such-vessel "$(python3 -c 'import json,sys; print(json.dumps({"id_unit": 999999, "latitude": -6.1, "longitude": 106.87, "recorded_at": sys.argv[1]}))' "$old_at")"

post unreadable '{"id_unit": 8, "latitude": "-6.1", "longitude": "106.87", "recorded_at": "not a time"}'

echo "done. The accepted 201 is not recorded here: it writes a fix for a real hull."
