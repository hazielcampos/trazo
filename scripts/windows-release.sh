#!/usr/bin/env bash
# Windows release: the app exe (what the updater downloads) and the installer.
#
#   bash scripts/windows-release.sh
#   -> target/release/Trazo-<version>-windows-x64.exe     (portable + update file)
#      target/release/Trazo-<version>-windows-setup.exe   (installer)
#      target/release/Trazo-<version>-windows.msi         (installer, .msi)
#
# The .msi needs WiX 5:  dotnet tool install --global wix --version 5.0.2
# (skipped with a warning when `wix` is missing).
#
# Upload both to the GitHub release. The updater looks for the asset whose
# name ends in "-windows-x64.exe", so keep that name.
set -euo pipefail
cd "$(dirname "$0")/.."
VERSION="$(grep -m1 '^version' Cargo.toml | cut -d'"' -f2)"

cargo build --release -p trazo_app
TRAZO_SETUP_PAYLOAD="$(pwd)/target/release/trazo.exe" cargo build --release -p trazo_setup

cp target/release/trazo.exe "target/release/Trazo-$VERSION-windows-x64.exe"
cp target/release/trazo-setup.exe "target/release/Trazo-$VERSION-windows-setup.exe"
if command -v wix >/dev/null; then
  EXE="$(pwd)/target/release/trazo.exe"
  command -v cygpath >/dev/null && EXE="$(cygpath -w "$EXE")"
  wix build scripts/windows/trazo.wxs -arch x64 -d "Version=$VERSION" -d "Exe=$EXE"     -o "target/release/Trazo-$VERSION-windows.msi"
  rm -f "target/release/Trazo-$VERSION-windows.wixpdb"
  echo "Listo: Trazo-$VERSION-windows-x64.exe, -windows-setup.exe y -windows.msi en target/release"
else
  echo "Aviso: sin WiX no hay .msi (dotnet tool install --global wix --version 5.0.2)"
  echo "Listo: Trazo-$VERSION-windows-x64.exe y -windows-setup.exe en target/release"
fi
