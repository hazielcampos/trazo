#!/usr/bin/env bash
# Build the release APK for phones, signed with Trazo's own key.
# The key lives outside the repository: ~/.trazo/trazo-release.jks (password in
# ~/.trazo/clave.txt). Every release must be signed with this same key, or
# Android won't install it as an update. Keep a backup of both files.
set -euo pipefail
cd "$(dirname "$0")/.."

KEY_DIR="${TRAZO_KEY_DIR:-$HOME/.trazo}"
[ -f "$KEY_DIR/trazo-release.jks" ] || { echo "No está la llave en $KEY_DIR"; exit 1; }
SDK="${LOCALAPPDATA}/Android/Sdk"
export ANDROID_HOME="$(cygpath -w "$SDK")"
export ANDROID_NDK_ROOT="$(cygpath -w "$(ls -d "$SDK"/ndk/* | sort -V | tail -1)")"
export JAVA_HOME="C:\Program Files\Android\Android Studio\jbr"
export CARGO_APK_RELEASE_KEYSTORE="$(cygpath -w "$KEY_DIR/trazo-release.jks")"
export CARGO_APK_RELEASE_KEYSTORE_PASSWORD="$(cat "$KEY_DIR/clave.txt")"

cargo apk build -p trazo_app --lib --target "${TARGET:-aarch64-linux-android}" --release
echo "Listo: target/release/apk/Trazo.apk"
