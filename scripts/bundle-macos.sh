#!/usr/bin/env bash
# Builds a universal (Apple Silicon + Intel) Redash.app and packages it as a zip and a dmg
# in target/dist/. Used by CI's release job; runs the same way locally.
#
# The app is ad-hoc signed, not notarized: on first launch macOS asks the user to allow it
# (right-click > Open, or System Settings > Privacy & Security).
set -euo pipefail
cd "$(dirname "$0")/.."

APP_NAME="Redash"
BIN="redash_desktop"
BUNDLE_ID="io.redash.desktop"
VERSION="$(cargo metadata --no-deps --format-version 1 \
  | python3 -c 'import json,sys; print(json.load(sys.stdin)["packages"][0]["version"])')"
TARGETS=(aarch64-apple-darwin x86_64-apple-darwin)
# Oldest macOS the binary runs on; also written to Info.plist.
export MACOSX_DEPLOYMENT_TARGET="11.0"

rustup target add "${TARGETS[@]}"
for target in "${TARGETS[@]}"; do
  cargo build --release --locked --target "$target"
done

DIST="target/dist"
APP="$DIST/$APP_NAME.app"
rm -rf "$DIST"
mkdir -p "$APP/Contents/MacOS" "$APP/Contents/Resources"

SLICES=()
for target in "${TARGETS[@]}"; do SLICES+=("target/$target/release/$BIN"); done
lipo -create -output "$APP/Contents/MacOS/$BIN" "${SLICES[@]}"
cp assets/fonts/Inter-LICENSE.txt "$APP/Contents/Resources/"

cat > "$APP/Contents/Info.plist" <<PLIST
<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
  <key>CFBundleName</key><string>$APP_NAME</string>
  <key>CFBundleDisplayName</key><string>$APP_NAME</string>
  <key>CFBundleIdentifier</key><string>$BUNDLE_ID</string>
  <key>CFBundleExecutable</key><string>$BIN</string>
  <key>CFBundlePackageType</key><string>APPL</string>
  <key>CFBundleShortVersionString</key><string>$VERSION</string>
  <key>CFBundleVersion</key><string>$VERSION</string>
  <key>CFBundleInfoDictionaryVersion</key><string>6.0</string>
  <key>LSMinimumSystemVersion</key><string>$MACOSX_DEPLOYMENT_TARGET</string>
  <key>NSHighResolutionCapable</key><true/>
  <key>LSApplicationCategoryType</key><string>public.app-category.developer-tools</string>
</dict>
</plist>
PLIST

codesign --force --deep --sign - "$APP"
codesign --verify --strict "$APP"

STEM="$APP_NAME-$VERSION-macos-universal"
ditto -c -k --keepParent "$APP" "$DIST/$STEM.zip"

STAGING="$DIST/dmg"
mkdir -p "$STAGING"
cp -R "$APP" "$STAGING/"
ln -s /Applications "$STAGING/Applications"
hdiutil create -volname "$APP_NAME" -srcfolder "$STAGING" -ov -format UDZO "$DIST/$STEM.dmg" >/dev/null
rm -rf "$STAGING"

echo "Built $DIST/$STEM.zip and $DIST/$STEM.dmg"
