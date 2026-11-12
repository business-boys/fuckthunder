#!/bin/bash
set -euo pipefail

ROOT="$(cd "$(dirname "$0")/../.." && pwd)"
cd "$ROOT"
if [[ "$(uname -s)" != Darwin ]]; then
    echo "Hakus.app must be built on macOS." >&2
    exit 1
fi
cargo build --locked --release -p fuckthunder-gui --bin fuckthunder-gui
APP="$ROOT/target/macos/Hakus.app"
mkdir -p "$APP/Contents/MacOS" "$APP/Contents/Resources"
cp target/release/fuckthunder-gui "$APP/Contents/MacOS/Hakus"
VERSION="$(cargo metadata --no-deps --format-version 1 | python3 -c 'import json,sys; print(next(p["version"] for p in json.load(sys.stdin)["packages"] if p["name"] == "fuckthunder-gui"))')"
cat > "$APP/Contents/Info.plist" <<EOF
<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0"><dict>
<key>CFBundleName</key><string>Hakus</string>
<key>CFBundleDisplayName</key><string>Hakus</string>
<key>CFBundleIdentifier</key><string>com.business-boys.hakus</string>
<key>CFBundleExecutable</key><string>Hakus</string>
<key>CFBundlePackageType</key><string>APPL</string>
<key>CFBundleShortVersionString</key><string>$VERSION</string>
<key>CFBundleVersion</key><string>$VERSION</string>
<key>CFBundleIconFile</key><string>Hakus</string>
<key>LSMinimumSystemVersion</key><string>12.0</string>
<key>LSApplicationCategoryType</key><string>public.app-category.productivity</string>
<key>NSHighResolutionCapable</key><true/>
<key>NSPrincipalClass</key><string>NSApplication</string>
</dict></plist>
EOF
ICONSET="$ROOT/target/macos/Hakus.iconset"
mkdir -p "$ICONSET"
swift "$ROOT/tools/macos/icon.swift" "$ICONSET"
iconutil -c icns "$ICONSET" -o "$APP/Contents/Resources/Hakus.icns"
plutil -lint "$APP/Contents/Info.plist"
codesign --force --sign - "$APP"
codesign --verify --deep --strict "$APP"
ditto -c -k --sequesterRsrc --keepParent "$APP" "$ROOT/target/macos/Hakus-macos-$(uname -m).zip"
printf '\nBuilt %s\n' "$APP"
if [[ "${1:-}" == --install ]]; then
    DEST="$HOME/Applications/Hakus.app"
    mkdir -p "$HOME/Applications"
    if [[ -e "$DEST" ]]; then
        BACKUP="$HOME/Applications/Hakus-backup-$(date +%Y%m%d-%H%M%S).app"
        mv "$DEST" "$BACKUP"
        printf 'Previous version: %s\n' "$BACKUP"
    fi
    ditto "$APP" "$DEST"
    /System/Library/Frameworks/CoreServices.framework/Frameworks/LaunchServices.framework/Support/lsregister -f "$DEST"
    printf 'Installed %s\nLaunch: open "%s"\n' "$DEST" "$DEST"
fi
