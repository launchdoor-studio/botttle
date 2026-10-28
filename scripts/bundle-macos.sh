#!/usr/bin/env bash
# Wraps the binary in a macOS .app so it gets a dock icon, a name in Cmd-Tab,
# and the logo instead of the generic terminal icon `cargo run` gives you.
#
#   scripts/bundle-macos.sh                      # release build, signed automatically
#   scripts/bundle-macos.sh --debug              # reuse the existing debug build
#   scripts/bundle-macos.sh --sign "identity"    # sign with a specific identity
#   scripts/bundle-macos.sh --no-sign            # leave it unsigned
#   scripts/bundle-macos.sh --notarize           # also notarize and staple it
#   scripts/bundle-macos.sh --install            # also copy it to /Applications
#
# Signing picks the best identity in the keychain: a Developer ID Application
# certificate if there is one (the only kind that can be notarized for other
# people's machines), otherwise an Apple Development certificate, otherwise an
# ad-hoc signature. All three are enough to run the app on this machine; only a
# notarized Developer ID build runs cleanly on someone else's — that is what
# --notarize adds, using the App Store Connect key `asc` already holds.
set -euo pipefail

root="$(cd "$(dirname "$0")/.." && pwd)"
profile="release"
identity="auto"
install=false
notarize=false

while [ $# -gt 0 ]; do
  case "$1" in
    --debug) profile="debug" ;;
    --sign) identity="$2"; shift ;;
    --no-sign) identity="" ;;
    --notarize) notarize=true ;;
    --install) install=true ;;
    *) echo "unknown option: $1" >&2; exit 2 ;;
  esac
  shift
done

app="$root/target/botttle.app"
version="$(sed -n 's/^version = "\(.*\)"/\1/p' "$root/Cargo.toml" | head -1)"

if [ "$profile" = "release" ]; then
  cargo build --release --manifest-path "$root/Cargo.toml"
else
  cargo build --manifest-path "$root/Cargo.toml"
fi

rm -rf "$app"
mkdir -p "$app/Contents/MacOS" "$app/Contents/Resources"
cp "$root/target/$profile/botttle" "$app/Contents/MacOS/botttle"

# The icon: every size macOS asks for, from the one source image.
#
# This is what the Dock and Finder show while botttle isn't running, so it is
# the twilight logo: what the default dynamic style picks for the default dark
# theme, and what the running app shows out of the box. It is clipped to
# Apple's icon grid the same way the running app clips it, so quitting doesn't
# change the icon's shape either.
iconset="$(mktemp -d)/botttle.iconset"
mkdir -p "$iconset"
icon_source="$(dirname "$iconset")/icon-source.png"
swift - "$root/assets/logo-twilight.png" "$icon_source" <<'SWIFT'
import AppKit

let arguments = CommandLine.arguments
guard let logo = NSImage(contentsOfFile: arguments[1]) else {
  FileHandle.standardError.write("cannot read \(arguments[1])\n".data(using: .utf8)!)
  exit(1)
}
// Apple's macOS icon grid: an 824-point rounded square centred on 1024.
let canvas = 1024, inset = 100.0, side = 824.0, corner = 185.4
let bitmap = NSBitmapImageRep(
  bitmapDataPlanes: nil, pixelsWide: canvas, pixelsHigh: canvas,
  bitsPerSample: 8, samplesPerPixel: 4, hasAlpha: true, isPlanar: false,
  colorSpaceName: .deviceRGB, bytesPerRow: 0, bitsPerPixel: 0)!
NSGraphicsContext.saveGraphicsState()
NSGraphicsContext.current = NSGraphicsContext(bitmapImageRep: bitmap)
let shape = NSRect(x: inset, y: inset, width: side, height: side)
NSBezierPath(roundedRect: shape, xRadius: corner, yRadius: corner).addClip()
logo.draw(in: shape)
NSGraphicsContext.restoreGraphicsState()
try! bitmap.representation(using: .png, properties: [:])!.write(to: URL(fileURLWithPath: arguments[2]))
SWIFT
for size in 16 32 128 256 512; do
  sips -Z "$size" "$icon_source" --out "$iconset/icon_${size}x${size}.png" >/dev/null
  sips -Z "$((size * 2))" "$icon_source" --out "$iconset/icon_${size}x${size}@2x.png" >/dev/null
done
if ! iconutil -c icns "$iconset" -o "$app/Contents/Resources/botttle.icns"; then
  # Some macOS installations reject even a complete, valid iconset. An ICNS
  # file can also contain the PNGs directly; build that form without extra tools.
  python3 - "$iconset" "$app/Contents/Resources/botttle.icns" <<'PY'
import pathlib
import struct
import sys

iconset = pathlib.Path(sys.argv[1])
output = pathlib.Path(sys.argv[2])
sizes = (
    (b'icp4', 'icon_16x16.png'),
    (b'icp5', 'icon_32x32.png'),
    (b'icp6', 'icon_32x32@2x.png'),
    (b'ic07', 'icon_128x128.png'),
    (b'ic08', 'icon_256x256.png'),
    (b'ic09', 'icon_512x512.png'),
    (b'ic10', 'icon_512x512@2x.png'),
)
chunks = []
for kind, filename in sizes:
    png = (iconset / filename).read_bytes()
    chunks.append(kind + struct.pack('>I', len(png) + 8) + png)
payload = b''.join(chunks)
output.write_bytes(b'icns' + struct.pack('>I', len(payload) + 8) + payload)
PY
fi
rm -rf "$(dirname "$iconset")"

cat > "$app/Contents/Info.plist" <<PLIST
<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
	<key>CFBundleName</key>
	<string>botttle</string>
	<key>CFBundleDisplayName</key>
	<string>botttle</string>
	<key>CFBundleExecutable</key>
	<string>botttle</string>
	<key>CFBundleIdentifier</key>
	<string>dev.botttle</string>
	<key>CFBundleIconFile</key>
	<string>botttle</string>
	<key>CFBundlePackageType</key>
	<string>APPL</string>
	<key>CFBundleShortVersionString</key>
	<string>$version</string>
	<key>CFBundleVersion</key>
	<string>$version</string>
	<key>LSMinimumSystemVersion</key>
	<string>11.0</string>
	<key>NSHighResolutionCapable</key>
	<true/>
</dict>
</plist>
PLIST

if [ "$identity" = "auto" ]; then
  names="$(security find-identity -v -p codesigning | sed -n 's/.*"\(.*\)".*/\1/p')"
  identity="$(printf '%s\n' "$names" | grep -m1 'Developer ID Application' || true)"
  [ -n "$identity" ] || identity="$(printf '%s\n' "$names" | grep -m1 'Apple Development' || true)"
  [ -n "$identity" ] || identity="-"
fi

if [ -n "$identity" ]; then
  # An ad-hoc signature has no certificate to timestamp against.
  if [ "$identity" = "-" ]; then
    timestamp="--timestamp=none"
  else
    timestamp="--timestamp"
  fi
  # The hardened runtime is what notarization will require later; nothing in a
  # terminal needs the exceptions it turns off.
  codesign --force --options runtime $timestamp --sign "$identity" "$app"
  codesign --verify --strict --verbose=2 "$app"
  echo "signed with: $identity"
fi

echo "built $app"

if $notarize; then
  if [ -z "$identity" ] || [ "$identity" = "-" ]; then
    echo "notarization needs a Developer ID Application signature" >&2
    exit 2
  fi
  zip="$root/target/botttle.zip"
  ditto -c -k --keepParent "$app" "$zip"
  asc notarization submit --file "$zip" --wait --output table
  # Stapling fails unless Apple actually accepted the submission, so this is
  # also the check that the notarization succeeded.
  xcrun stapler staple "$app"
  spctl -a -t exec -vv "$app"
fi

if $install; then
  destination="/Applications/botttle.app"
  rm -rf "$destination"
  cp -R "$app" "$destination"
  echo "installed $destination"
  echo "open it with:  open -a botttle"
else
  echo "run it with:   open $app"
fi
