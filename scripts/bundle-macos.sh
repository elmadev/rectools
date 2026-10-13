#!/bin/sh
# Builds recview.app, which opens .rec files from Finder.
# Usage: scripts/bundle-macos.sh [--install]
#   --install  also copy it to ~/Applications
set -eu

cd "$(dirname "$0")/.."
export PATH="/opt/homebrew/opt/rustup/bin:/usr/local/opt/rustup/bin:$HOME/.cargo/bin:$PATH"

cargo build --release -p recview

VERSION=$(sed -n 's/^version = "\(.*\)"/\1/p' Cargo.toml | head -n 1)
APP=target/release/recview.app
rm -rf "$APP"
mkdir -p "$APP/Contents/MacOS" "$APP/Contents/Resources"
cp target/release/recview "$APP/Contents/MacOS/recview"

cat > "$APP/Contents/Info.plist" <<PLIST
<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
    <key>CFBundleName</key>
    <string>recview</string>
    <key>CFBundleDisplayName</key>
    <string>recview</string>
    <key>CFBundleIdentifier</key>
    <string>com.rectools.recview</string>
    <key>CFBundleExecutable</key>
    <string>recview</string>
    <key>CFBundlePackageType</key>
    <string>APPL</string>
    <key>CFBundleShortVersionString</key>
    <string>$VERSION</string>
    <key>CFBundleVersion</key>
    <string>$VERSION</string>
    <key>LSMinimumSystemVersion</key>
    <string>11.0</string>
    <key>NSHighResolutionCapable</key>
    <true/>
    <key>UTImportedTypeDeclarations</key>
    <array>
        <dict>
            <key>UTTypeIdentifier</key>
            <string>com.elastomania.rec</string>
            <key>UTTypeDescription</key>
            <string>Elasto Mania replay</string>
            <key>UTTypeConformsTo</key>
            <array>
                <string>public.data</string>
            </array>
            <key>UTTypeTagSpecification</key>
            <dict>
                <key>public.filename-extension</key>
                <array>
                    <string>rec</string>
                    <string>REC</string>
                </array>
            </dict>
        </dict>
        <dict>
            <key>UTTypeIdentifier</key>
            <string>com.elastomania.lev</string>
            <key>UTTypeDescription</key>
            <string>Elasto Mania level</string>
            <key>UTTypeConformsTo</key>
            <array>
                <string>public.data</string>
            </array>
            <key>UTTypeTagSpecification</key>
            <dict>
                <key>public.filename-extension</key>
                <array>
                    <string>lev</string>
                    <string>LEV</string>
                </array>
            </dict>
        </dict>
    </array>
    <key>CFBundleDocumentTypes</key>
    <array>
        <dict>
            <key>CFBundleTypeName</key>
            <string>Elasto Mania replay</string>
            <key>CFBundleTypeRole</key>
            <string>Viewer</string>
            <key>LSHandlerRank</key>
            <string>Owner</string>
            <key>LSItemContentTypes</key>
            <array>
                <string>com.elastomania.rec</string>
                <!-- VLC claims .rec as a video type; files get that type when it is installed -->
                <string>org.videolan.rec</string>
            </array>
        </dict>
        <dict>
            <key>CFBundleTypeName</key>
            <string>Elasto Mania level</string>
            <key>CFBundleTypeRole</key>
            <string>Viewer</string>
            <!-- Offered in Open With, without taking levels over from an editor -->
            <key>LSHandlerRank</key>
            <string>Alternate</string>
            <key>LSItemContentTypes</key>
            <array>
                <string>com.elastomania.lev</string>
            </array>
        </dict>
    </array>
</dict>
</plist>
PLIST

# An ad-hoc signature, so macOS accepts the bundle on this machine
codesign --force --sign - "$APP"
echo "Built $APP"

if [ "${1:-}" = "--install" ]; then
    mkdir -p "$HOME/Applications"
    rm -rf "$HOME/Applications/recview.app"
    cp -R "$APP" "$HOME/Applications/"
    echo "Installed $HOME/Applications/recview.app"
fi
