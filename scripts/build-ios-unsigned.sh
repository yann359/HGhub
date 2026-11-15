#!/usr/bin/env bash
set -euo pipefail
VERSION="${1:-1.0.0}"
[[ "$(uname -s)" == Darwin ]] || { echo 'Xcode on macOS is required'; exit 1; }
[[ "$VERSION" =~ ^[0-9]+\.[0-9]+\.[0-9]+$ ]] || exit 1
ROOT="$(cd "$(dirname "$0")/.." && pwd)"
cd "$ROOT"
mkdir -p artifacts/logs
# All tool output is retained, including failures.
exec > >(tee "$ROOT/artifacts/logs/ios-build.log") 2>&1
export IPHONEOS_DEPLOYMENT_TARGET=16.4
export HGHUB_IOS_VERSION="$VERSION"
export LIBCLANG_PATH="$(xcode-select -p)/Toolchains/XcodeDefault.xctoolchain/usr/lib"
# No fake Team ID and no signing assets. tauri 2.10.1 has no ios --no-sign.
# --open starts the Release options server used by the generated Rust build phase.
IDENTIFIER="$(node -p "require('./src-tauri/tauri.ios.conf.json').identifier")"
# Tauri keys the options server by the BASE identifier, before iOS overrides.
BASE_IDENTIFIER="$(node -p "require('./src-tauri/tauri.conf.json').identifier")"
ADDR_FILE="$(node -p "require('os').tmpdir()")/$BASE_IDENTIFIER-server-addr"
rm -f "$ADDR_FILE"
pnpm tauri ios build --open --ci --features custom-protocol -- --locked > artifacts/logs/tauri-session.log 2>&1 &
SESSION_PID=$!
cleanup() { kill "$SESSION_PID" 2>/dev/null || true; cat artifacts/logs/tauri-session.log; }
trap cleanup EXIT
for ((attempt=0; attempt<240; attempt++)); do
  kill -0 "$SESSION_PID" 2>/dev/null || { echo 'Tauri options server exited'; exit 1; }
  [[ ! -s "$ADDR_FILE" ]] || break
  sleep 1
done
[[ -s "$ADDR_FILE" ]] || { echo 'Timed out waiting for Tauri options server'; exit 1; }
xcodebuild build \
  -project src-tauri/gen/apple/huangguo-client.xcodeproj \
  -scheme huangguo-client_iOS -configuration release \
  -sdk iphoneos -destination 'generic/platform=iOS' \
  -derivedDataPath "$ROOT/build_output" \
  -resultBundlePath "$ROOT/build_output/ios-release.xcresult" \
  ARCHS=arm64 ONLY_ACTIVE_ARCH=NO \
  CODE_SIGNING_ALLOWED=NO CODE_SIGNING_REQUIRED=NO CODE_SIGN_IDENTITY="" DEVELOPMENT_TEAM="" \
  | tee artifacts/logs/xcodebuild.log
APP="$ROOT/build_output/Build/Products/release-iphoneos/HGhub.app"
[[ -d "$APP" ]] || { echo 'Expected Release iPhone app missing'; exit 1; }
STAGE="$(mktemp -d)"
trap 'rm -rf "$STAGE"; cleanup' EXIT
mkdir "$STAGE/Payload"
# Preserve executable modes, bundle directories and symlinks.
ditto "$APP" "$STAGE/Payload/HGhub.app"
IPA="$ROOT/artifacts/HGhub-iOS-LiquidGlass-v$VERSION.ipa"
(cd "$STAGE" && /usr/bin/zip -qry "$IPA" Payload)
python3 scripts/verify-ipa.py "$IPA" --bundle-id "$IDENTIFIER" --version "$VERSION" \
  --manifest artifacts/ios-build-manifest.json
(cd artifacts && shasum -a 256 "$(basename "$IPA")" > SHA256SUMS.txt)
