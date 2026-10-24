#!/usr/bin/env bash
# Build the Android APK, start the tablet emulator if needed, install and launch.
# Usage: scripts/android.sh [release]   (debug by default)
set -euo pipefail
cd "$(dirname "$0")/.."

SDK="${LOCALAPPDATA}/Android/Sdk"
export ANDROID_HOME="$(cygpath -w "$SDK")"
NDK_DIR="$(ls -d "$SDK"/ndk/* | sort -V | tail -1)"
export ANDROID_NDK_ROOT="$(cygpath -w "$NDK_DIR")"
export JAVA_HOME="C:\Program Files\Android\Android Studio\jbr"
ADB="$SDK/platform-tools/adb.exe"
EMU="$SDK/emulator/emulator.exe"
AVD="${AVD:-Animate_Tablet}"
PROFILE_FLAG=""; OUT="debug"
if [ "${1:-}" = "release" ]; then PROFILE_FLAG="--release"; OUT="release"; fi

# The emulator image is x86_64; real tablets are aarch64.
TARGET="${TARGET:-x86_64-linux-android}"
cargo apk build -p trazo_app --lib --target "$TARGET" $PROFILE_FLAG

if ! "$ADB" devices | grep -q "emulator-"; then
  echo "Arrancando el emulador $AVD…"
  "$EMU" -avd "$AVD" -gpu auto >/dev/null 2>&1 &
fi
# With a phone also plugged in, use the emulator unless ANDROID_SERIAL says otherwise.
until "$ADB" devices | grep -q "emulator-"; do sleep 2; done
export ANDROID_SERIAL="${ANDROID_SERIAL:-$("$ADB" devices | grep -o 'emulator-[0-9]*' | head -1)}"
"$ADB" wait-for-device
until [ "$("$ADB" shell getprop sys.boot_completed 2>/dev/null | tr -d '\r')" = "1" ]; do sleep 2; done

"$ADB" install -r "target/$OUT/apk/Trazo.apk"
"$ADB" shell am force-stop com.trazo.app || true
"$ADB" shell am start -n com.trazo.app/android.app.NativeActivity
echo "Trazo abierto en $AVD"
