#!/usr/bin/env bash
# Windows release: the app exe (what the updater downloads) and the installer.
#
#   bash scripts/windows-release.sh
#   -> target/release/Trazo-<version>-windows-x64.exe     (portable + update file)
#      target/release/Trazo-<version>-windows-setup.exe   (installer)
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
echo "Listo: target/release/Trazo-$VERSION-windows-x64.exe y Trazo-$VERSION-windows-setup.exe"
