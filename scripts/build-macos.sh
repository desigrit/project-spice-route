#!/usr/bin/env bash
set -euo pipefail

target="${1:-}"
version="${2:-}"
artifact_arch="${3:-}"

case "$target" in
  aarch64-apple-darwin)
    expected_arch="arm64"
    ;;
  x86_64-apple-darwin)
    expected_arch="x86_64"
    ;;
  *)
    echo "Use aarch64-apple-darwin or x86_64-apple-darwin." >&2
    exit 2
    ;;
esac

if [[ ! "$version" =~ ^[0-9]+\.[0-9]+\.[0-9]+$ ]]; then
  echo "Provide a release version such as 1.5.2." >&2
  exit 2
fi
if [[ ! "$artifact_arch" =~ ^[a-z0-9-]+$ ]]; then
  echo "Provide a lowercase artifact architecture label." >&2
  exit 2
fi
if [[ "$(uname -s)" != "Darwin" ]]; then
  echo "Build the macOS package on macOS." >&2
  exit 2
fi

repository_root="$(cd "$(dirname "$0")/.." && pwd -P)"
cd "$repository_root"

rustup target add "$target"

# An ad-hoc signature keeps the Apple Silicon bundle structurally valid. It does
# not provide developer identity or notarization, so CI names the output clearly.
export APPLE_SIGNING_IDENTITY="-"
config_override="{\"version\":\"${version}\"}"
npm run tauri build -- --ci --target "$target" --bundles app,dmg --config "$config_override"

bundle_root="$repository_root/src-tauri/target/$target/release/bundle"
app_path="$bundle_root/macos/Spice Route.app"
if [[ ! -d "$app_path" ]]; then
  app_path="$(find "$bundle_root/macos" -maxdepth 1 -type d -name '*.app' -print -quit)"
fi
if [[ -z "$app_path" || ! -d "$app_path" ]]; then
  echo "The macOS application bundle was not produced." >&2
  exit 1
fi

main_binary="$(find "$app_path/Contents/MacOS" -maxdepth 1 -type f -perm -111 -print -quit)"
if [[ -z "$main_binary" || ! -f "$main_binary" ]]; then
  echo "The application executable was not found in the bundle." >&2
  exit 1
fi
binary_archs="$(lipo -archs "$main_binary")"
if [[ " $binary_archs " != *" $expected_arch "* ]]; then
  echo "The application executable does not contain $expected_arch code: $binary_archs" >&2
  exit 1
fi
codesign --verify --deep --strict "$app_path"
entitlements_dump="$bundle_root/SpiceRoute.entitlements.plist"
codesign -d --entitlements :- "$app_path" > "$entitlements_dump" 2>/dev/null
python3 - "$entitlements_dump" <<'PY'
import plistlib
import sys

with open(sys.argv[1], "rb") as source:
    entitlements = plistlib.load(source)
if entitlements.get("com.apple.security.automation.apple-events") is not True:
    raise SystemExit("The application bundle is missing its Apple Events automation entitlement.")
PY
if [[ -z "$(plutil -extract NSAppleEventsUsageDescription raw -o - "$app_path/Contents/Info.plist")" ]]; then
  echo "The application bundle is missing its Apple Events usage description." >&2
  exit 1
fi

dmg_path="$(find "$bundle_root/dmg" -maxdepth 1 -type f -name '*.dmg' -print -quit)"
if [[ -z "$dmg_path" || ! -f "$dmg_path" ]]; then
  echo "The macOS disk image was not produced." >&2
  exit 1
fi

artifact_dir="$repository_root/artifacts/macos-$artifact_arch"
rm -rf "$artifact_dir"
mkdir -p "$artifact_dir"

artifact_base="Spice-Route-$version-macos-$artifact_arch"
cp "$dmg_path" "$artifact_dir/$artifact_base.dmg"
ditto -c -k --sequesterRsrc --keepParent "$app_path" "$artifact_dir/$artifact_base.app.zip"

(
  cd "$artifact_dir"
  shasum -a 256 "$artifact_base.dmg" "$artifact_base.app.zip" > SHA256SUMS.txt
)

echo "Ad-hoc signed app archive: $artifact_dir/$artifact_base.app.zip"
echo "Ad-hoc signed disk image: $artifact_dir/$artifact_base.dmg"
echo "These packages are not notarized and may require approval in Privacy & Security."
