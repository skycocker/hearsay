#!/usr/bin/env bash
set -euo pipefail

# Build a macOS .app bundle for hearsay.
# Usage: ./scripts/bundle-macos.sh
# Output: target/release/Hearsay.app

cd "$(dirname "$0")/.."

echo "==> Building the frontend"
(cd ui-frontend && npm install --silent && npm run build)

echo "==> Building the release binary (tray feature)"
cargo build --release -p hearsayd --features tray

APP="target/release/Hearsay.app"
rm -rf "$APP"
mkdir -p "$APP/Contents/MacOS"
mkdir -p "$APP/Contents/Resources"

cp target/release/hearsayd "$APP/Contents/MacOS/hearsayd"

# CFBundleIconFile is omitted intentionally — the tray icon is synthesized
# at runtime in src/tray.rs::make_icon. The .app uses the system default
# until someone adds an icon.icns.

cat > "$APP/Contents/Info.plist" <<'PLIST'
<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
    <key>CFBundleName</key>
    <string>Hearsay</string>
    <key>CFBundleDisplayName</key>
    <string>Hearsay</string>
    <key>CFBundleIdentifier</key>
    <string>io.hearsay.app</string>
    <key>CFBundleVersion</key>
    <string>0.0.1</string>
    <key>CFBundleShortVersionString</key>
    <string>0.0.1</string>
    <key>CFBundleExecutable</key>
    <string>hearsayd</string>
    <key>CFBundlePackageType</key>
    <string>APPL</string>
    <key>LSMinimumSystemVersion</key>
    <string>13.0</string>
    <key>NSHighResolutionCapable</key>
    <true/>
    <!-- Menubar-only — no Dock icon. -->
    <key>LSUIElement</key>
    <true/>
    <!-- TCC permission strings; shown when macOS prompts the user. -->
    <key>NSMicrophoneUsageDescription</key>
    <string>Hearsay records meeting audio from the microphone you pick in the web UI.</string>
</dict>
</plist>
PLIST

echo "==> Built: $APP"
echo "    Run: open $APP"
echo "    Or:  $APP/Contents/MacOS/hearsayd"
