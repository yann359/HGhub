#!/usr/bin/env bash
set -euo pipefail
APP="${1:?Pass the release simulator app bundle}"
IDENTIFIER="${2:?Pass the simulator bundle identifier}"
LOG=artifacts/logs
xcrun simctl list devices available -j > "$LOG/simulator-devices.json"
CHOICE="$(python3 - "$LOG/simulator-devices.json" <<'PY'
import json, sys
data = json.load(open(sys.argv[1], encoding="utf-8"))
candidates = [
    (tuple(map(int, runtime.split(".iOS-")[-1].split("-"))), d["name"] == "iPhone 17 Pro Max", d["name"], d["udid"])
    for runtime, devices in data["devices"].items() if ".iOS-" in runtime
    for d in devices if d.get("isAvailable") and d["name"].startswith("iPhone")
]
if not candidates:
    sys.exit("No iOS simulator available")
version, _, name, udid = max(candidates)
print(udid)
print(f"{name} / iOS {'.'.join(map(str, version))}", file=sys.stderr)
PY
)"
xcrun simctl boot "$CHOICE"
trap 'xcrun simctl shutdown "$CHOICE" || true' EXIT
xcrun simctl bootstatus "$CHOICE" -b
codesign --force --sign - "$APP"
xcrun simctl install "$CHOICE" "$APP"
xcrun simctl launch "$CHOICE" "$IDENTIFIER" | tee "$LOG/simulator-launch.txt"
DATA="$(xcrun simctl get_app_container "$CHOICE" "$IDENTIFIER" data)"
verify_index() {
  python3 - "$DATA" <<'PY'
import pathlib, sys
root = pathlib.Path(sys.argv[1])
receipts = {p.name for p in root.glob("**/hpp/bundled-installed/*") if p.is_file()}
expected = {"dev.hghub.blender-open-movies", "dev.hghub.nasa-svs", "dev.hghub.nasa-svs-resolver"}
if not expected <= receipts:
    sys.exit(1)
for index in root.glob("**/hpp/index.json"):
    content = index.read_text(encoding="utf-8")
    if all(id in content for id in expected):
        print(index)
        sys.exit(0)
sys.exit(1)
PY
}
capture() {
  xcrun simctl io "$CHOICE" screenshot "$LOG/simulator-home.png" || true
  find "$DATA" -name debug.log -exec cp {} "$LOG/simulator-debug.log" \; || true
}
for ((attempt=0; attempt<30; attempt++)); do
  if verify_index > "$LOG/simulator-data.txt"; then break; fi
  sleep 2
done
capture
test -s "$LOG/simulator-data.txt"
xcrun simctl terminate "$CHOICE" "$IDENTIFIER"
LAUNCH="$(xcrun simctl launch "$CHOICE" "$IDENTIFIER")"
printf "%s\n" "$LAUNCH" | tee -a "$LOG/simulator-launch.txt"
PID="${LAUNCH##*: }"
for ((attempt=0; attempt<24; attempt++)); do
  log="$(find "$DATA" -name debug.log -print -quit)"
  if [[ -n "$log" ]] && grep -F "[pid=$PID] [player] source.first-frame" "$log"; then break; fi
  sleep 2
done
verify_index >> "$LOG/simulator-data.txt"
capture
test -f "$LOG/simulator-debug.log"
grep -F "[pid=$PID] [player] source.first-frame" "$LOG/simulator-debug.log"
