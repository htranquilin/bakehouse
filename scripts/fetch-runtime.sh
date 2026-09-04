#!/usr/bin/env bash
# Fetch the pinned apple/container release and stage its (pre-signed) binaries
# into src-tauri/resources/container-runtime/, preserving Apple's bin/../libexec
# layout that the CLI uses for plugin discovery.
#
# The binaries ship signed by Apple with com.apple.security.virtualization on
# container-runtime-linux; we bundle them UNMODIFIED so no re-signing is needed
# (verified in spike/findings.md).

set -euo pipefail

VERSION="1.3.1"
PKG_URL="https://github.com/apple/container/releases/download/${VERSION}/container-${VERSION}-installer-signed.pkg"

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
DEST="$ROOT/src-tauri/resources/container-runtime"
CACHE="$ROOT/.cache"
PKG="$CACHE/container-${VERSION}.pkg"

mkdir -p "$CACHE"
if [[ ! -f "$PKG" ]]; then
  echo "Downloading container ${VERSION}..."
  curl -fL --progress-bar -o "$PKG" "$PKG_URL"
fi

pkgutil --check-signature "$PKG" | grep -q "trusted by the Apple notary service" \
  || { echo "ERROR: pkg signature/notarization check failed"; exit 1; }

WORK="$(mktemp -d)"
trap 'rm -rf "$WORK"' EXIT
pkgutil --expand-full "$PKG" "$WORK/expanded"

rm -rf "$DEST"
mkdir -p "$DEST"
cp -R "$WORK/expanded/Payload/bin" "$DEST/bin"
cp -R "$WORK/expanded/Payload/libexec" "$DEST/libexec"

# Not needed by Bakehouse: installer helper scripts and the k8s plugin (61 MB).
rm -f "$DEST/bin/uninstall-container.sh" "$DEST/bin/update-container.sh"
rm -rf "$DEST/libexec/container/plugins/k8s"

find "$DEST" -type f -path "*/bin/*" -exec chmod +x {} \;

# Sanity: signatures must still validate after relocation.
codesign -v "$DEST/bin/container" "$DEST/bin/container-apiserver" \
  "$DEST/libexec/container/plugins/container-runtime-linux/bin/container-runtime-linux"
codesign -d --entitlements - "$DEST/libexec/container/plugins/container-runtime-linux/bin/container-runtime-linux" 2>&1 \
  | grep -q "com.apple.security.virtualization" \
  || { echo "ERROR: virtualization entitlement missing"; exit 1; }

echo "Runtime staged at $DEST ($(du -sh "$DEST" | cut -f1))"
