#!/usr/bin/env sh
# Installs QRnew for the current user. No root required.
#   ./install.sh            -> installs into ~/.local
#   ./install.sh /usr/local -> installs into /usr/local (needs sudo)
set -eu

APPID=dev.lhdjung.QRnew
BIN=qrnew
PREFIX="${1:-$HOME/.local}"

here=$(CDPATH= cd -- "$(dirname -- "$0")" && pwd)

install -Dm0755 "$here/$BIN"                "$PREFIX/bin/$BIN"
install -Dm0644 "$here/$APPID.desktop"      "$PREFIX/share/applications/$APPID.desktop"
install -Dm0644 "$here/$APPID.metainfo.xml" "$PREFIX/share/metainfo/$APPID.metainfo.xml"
install -Dm0644 "$here/$APPID.svg"          "$PREFIX/share/icons/hicolor/scalable/apps/$APPID.svg"

update-desktop-database "$PREFIX/share/applications" 2>/dev/null || true
gtk-update-icon-cache -f -t "$PREFIX/share/icons/hicolor" 2>/dev/null || true

printf 'QRnew installed to %s\n' "$PREFIX"
printf 'It should now be in your application launcher. Run ./uninstall.sh to remove it.\n'

case ":${PATH}:" in
    *":$PREFIX/bin:"*) ;;
    *) printf '\nNote: %s/bin is not on your PATH, so the `qrnew` command will not work in a terminal.\n' "$PREFIX" ;;
esac
