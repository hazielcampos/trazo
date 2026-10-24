#!/usr/bin/env bash
# Rebuild crates/trazo_app/android/picker.dex from TrazoPicker.java.
# The dex is committed; run this only after editing the Java file.
set -euo pipefail
cd "$(dirname "$0")/../crates/trazo_app/android"

SDK="${LOCALAPPDATA}/Android/Sdk"
PLATFORM="$(ls -d "$SDK"/platforms/android-* | sort -V | tail -1)"
BUILD_TOOLS="$(ls -d "$SDK"/build-tools/* | sort -V | tail -1)"
JAVAC="/c/Program Files/Android/Android Studio/jbr/bin/javac.exe"

rm -rf build && mkdir -p build
"$JAVAC" --release 11 -classpath "$(cygpath -w "$PLATFORM/android.jar")" -d build TrazoPicker.java
"$BUILD_TOOLS/d8.bat" --release --min-api 29 --lib "$(cygpath -w "$PLATFORM/android.jar")" --output build $(find build -name '*.class' | while read -r f; do cygpath -w "$f"; done)
mv build/classes.dex picker.dex
rm -rf build
echo "Listo: crates/trazo_app/android/picker.dex"
