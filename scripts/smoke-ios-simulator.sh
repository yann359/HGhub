#!/usr/bin/env bash
set -euo pipefail
APP="${1:?Pass the release simulator app bundle}"
IDENTIFIER="${2:?Pass the simulator bundle identifier}"
LOG=artifacts/logs
simctl() {
  python3 - "$@" <<'PY'
import os, subprocess, sys
subprocess.run(["xcrun", "simctl", *sys.argv[1:]], check=True, timeout=int(os.environ.get("HGHUB_SIM_WAIT", "30")))
PY
}
simctl list devices available -j > "$LOG/simulator-devices.json"
CHOICE="$(python3 - "$LOG/simulator-devices.json" <<'PY'
import json, re, sys
data = json.load(open(sys.argv[1], encoding="utf-8"))
candidates = [
    (version[0] >= 27, d["name"] == "iPhone 17 Pro Max", d["name"].endswith("Pro Max"), version, d["name"], d["udid"])
    for runtime, devices in data["devices"].items()
    if (match := re.search(r"\.iOS-(\d+(?:-\d+)*)", runtime))
    for version in [tuple(map(int, match[1].split("-")))]
    for d in devices if d.get("isAvailable") and d["name"].startswith("iPhone")
]
if not candidates:
    sys.exit("No iOS simulator available")
_, _, _, version, name, udid = max(candidates)
print(udid)
print(f"{name} / iOS {'.'.join(map(str, version))}", file=sys.stderr)
PY
)"
trap 'HGHUB_SIM_WAIT=15 simctl shutdown "$CHOICE" || true' EXIT
echo "Starting simulator"
simctl boot "$CHOICE"
HGHUB_SIM_WAIT=150 simctl bootstatus "$CHOICE" -b
codesign --force --sign - "$APP"
simctl install "$CHOICE" "$APP"
simctl launch "$CHOICE" "$IDENTIFIER" | tee "$LOG/simulator-launch.txt"
DATA="$(simctl get_app_container "$CHOICE" "$IDENTIFIER" data)"
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
  simctl io "$CHOICE" screenshot "$LOG/simulator-home.png" || true
  find "$DATA" -name debug.log -exec cp {} "$LOG/simulator-debug.log" \; || true
}
for ((attempt=0; attempt<30; attempt++)); do
  if verify_index > "$LOG/simulator-data.txt"; then break; fi
  sleep 2
done
capture
test -s "$LOG/simulator-data.txt"
simctl terminate "$CHOICE" "$IDENTIFIER"
LAUNCH="$(simctl launch "$CHOICE" "$IDENTIFIER")"
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
