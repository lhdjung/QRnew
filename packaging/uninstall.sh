#!/usr/bin/env sh
# Removes a QRnew install created by install.sh.
set -eu

APPID=dev.lhdjung.QRnew
BIN=qrnew
PREFIX="${1:-$HOME/.local}"

rm -f "$PREFIX/bin/$BIN" \
      "$PREFIX/share/applications/$APPID.desktop" \
      "$PREFIX/share/metainfo/$APPID.metainfo.xml" \
      "$PREFIX/share/icons/hicolor/scalable/apps/$APPID.svg"

update-desktop-database "$PREFIX/share/applications" 2>/dev/null || true

printf 'QRnew removed from %s\n' "$PREFIX"
