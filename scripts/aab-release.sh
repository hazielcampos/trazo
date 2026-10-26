#!/usr/bin/env bash
# Build the Android App Bundle (.aab) for Google Play, signed with Trazo's own key.
#
#   bash scripts/aab-release.sh            -> target/release/Trazo-<version>.aab
#   SKIP_BUILD=1 bash scripts/aab-release.sh   (reuse the per-ABI APKs already built)
#
# cargo-apk only makes APKs, so this builds one APK per ABI (arm64 + x86_64; add armv7
# with `rustup target add armv7-linux-androideabi` and ABIS="aarch64-linux-android armv7-linux-androideabi x86_64-linux-android"),
# takes the native libraries out of them, compiles the manifest and resources in
# protobuf form with aapt2 and assembles the bundle with bundletool.
#
# Needs: the key in ~/.trazo (see apk-release.sh) and bundletool at
# ~/.trazo/herramientas/bundletool.jar (https://github.com/google/bundletool/releases).
set -euo pipefail
cd "$(dirname "$0")/.."

KEY_DIR="${TRAZO_KEY_DIR:-$HOME/.trazo}"
BUNDLETOOL="${BUNDLETOOL:-$KEY_DIR/herramientas/bundletool.jar}"
[ -f "$KEY_DIR/trazo-release.jks" ] || { echo "No está la llave en $KEY_DIR"; exit 1; }
[ -f "$BUNDLETOOL" ] || { echo "Falta bundletool en $BUNDLETOOL"; exit 1; }
SDK="${LOCALAPPDATA}/Android/Sdk"
BT="$(ls -d "$SDK"/build-tools/* | sort -V | tail -1)"
PLATFORM="$(ls -d "$SDK"/platforms/android-* | sort -V | tail -1)"
JBR="/c/Program Files/Android/Android Studio/jbr/bin"
ABIS="${ABIS:-aarch64-linux-android x86_64-linux-android}"
VERSION="$(grep -m1 '^version' Cargo.toml | cut -d'"' -f2)"
W() { cygpath -w "$1"; }

OUT="target/release/aab"
rm -rf "$OUT" && mkdir -p "$OUT/base/lib"

# 1. One APK per ABI; keep its native library.
for T in $ABIS; do
  if [ -z "${SKIP_BUILD:-}" ]; then
    TARGET="$T" bash scripts/apk-release.sh
    cp target/release/apk/Trazo.apk "target/release/apk/Trazo-$T.apk"
  fi
  python -c "import sys,zipfile; z=zipfile.ZipFile(sys.argv[1]); [z.extract(n, sys.argv[2]) for n in z.namelist() if n.startswith('lib/')]" "target/release/apk/Trazo-$T.apk" "$OUT/base"
done

# 2. Manifest and resources in protobuf form.
"$BT/aapt2.exe" compile --dir "$(W assets/icon/android-res)" -o "$(W "$OUT/res.zip")"
"$BT/aapt2.exe" link --proto-format -o "$(W "$OUT/proto.apk")" -I "$(W "$PLATFORM/android.jar")" \
  --manifest "$(W target/release/apk/AndroidManifest.xml)" --auto-add-overlay "$(W "$OUT/res.zip")"
python -c "import sys,zipfile; zipfile.ZipFile(sys.argv[1]).extractall(sys.argv[2])" "$OUT/proto.apk" "$OUT/base"
(cd "$OUT/base" && mkdir -p manifest && mv AndroidManifest.xml manifest/)

# 3. Module zip -> bundle.
(cd "$OUT/base" && rm -f ../base.zip && "$JBR/jar.exe" cfM ../base.zip -C . .)
AAB="target/release/Trazo-$VERSION.aab"
rm -f "$AAB"
"$JBR/java.exe" -jar "$(W "$BUNDLETOOL")" build-bundle --modules="$(W "$OUT/base.zip")" --output="$(W "$AAB")"

# 4. Sign with the release key.
"$JBR/jarsigner.exe" -keystore "$(W "$KEY_DIR/trazo-release.jks")" -storepass "$(cat "$KEY_DIR/clave.txt")" \
  -sigalg SHA256withRSA -digestalg SHA-256 "$(W "$AAB")" trazo >/dev/null
"$JBR/java.exe" -jar "$(W "$BUNDLETOOL")" validate --bundle="$(W "$AAB")" >/dev/null
echo "Listo: $AAB"
