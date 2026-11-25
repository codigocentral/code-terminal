#!/usr/bin/env bash
set -euo pipefail

repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$repo_root"

version="$(sed -nE 's/^version[[:space:]]*=[[:space:]]*"([^"]+)".*/\1/p' Cargo.toml | head -n 1)"
if [[ -z "$version" ]]; then
  echo "Nao foi possivel ler a versao do Cargo.toml." >&2
  exit 1
fi

arch="$(uname -m)"
case "$arch" in
  x86_64) deb_arch="amd64" ;;
  aarch64|arm64) deb_arch="arm64" ;;
  *) deb_arch="$arch" ;;
esac

cargo build --release --bins -j 1

dist_dir="$repo_root/dist"
stage_dir="$repo_root/target/package/linux/code-terminal-$version-$arch"
deb_root="$repo_root/target/package/linux/deb-root"

rm -rf "$stage_dir" "$deb_root"
mkdir -p "$dist_dir" "$stage_dir/bin" "$stage_dir/share/applications" "$stage_dir/share/systemd/user"

install -m 755 target/release/code-terminal "$stage_dir/bin/code-terminal"
install -m 755 target/release/code-terminal-cli "$stage_dir/bin/code-terminal-cli"
install -m 755 target/release/code-terminal-browser "$stage_dir/bin/code-terminal-browser"

cat > "$stage_dir/share/applications/code-terminal.desktop" <<EOF
[Desktop Entry]
Type=Application
Name=Code Terminal
Comment=Modern terminal emulator
Exec=code-terminal
Terminal=false
Categories=System;TerminalEmulator;
EOF

# Modo servidor (opcional): `systemctl --user enable --now code-terminal-server`.
install -m 644 packaging/linux/code-terminal-server.service "$stage_dir/share/systemd/user/code-terminal-server.service"
install -m 644 packaging/linux/code-terminal-mux.service "$stage_dir/share/systemd/user/code-terminal-mux.service"

tarball="$dist_dir/code-terminal-$version-linux-$arch.tar.gz"
tar -C "$(dirname "$stage_dir")" -czf "$tarball" "$(basename "$stage_dir")"
echo "Arquivo tar.gz gerado: $tarball"

if command -v dpkg-deb >/dev/null 2>&1; then
  mkdir -p "$deb_root/DEBIAN" "$deb_root/usr/bin" "$deb_root/usr/share/applications" "$deb_root/usr/lib/systemd/user"
  install -m 755 target/release/code-terminal "$deb_root/usr/bin/code-terminal"
  install -m 755 target/release/code-terminal-cli "$deb_root/usr/bin/code-terminal-cli"
  install -m 755 target/release/code-terminal-browser "$deb_root/usr/bin/code-terminal-browser"
  install -m 644 "$stage_dir/share/applications/code-terminal.desktop" "$deb_root/usr/share/applications/code-terminal.desktop"
  install -m 644 packaging/linux/code-terminal-server.service "$deb_root/usr/lib/systemd/user/code-terminal-server.service"
  install -m 644 packaging/linux/code-terminal-mux.service "$deb_root/usr/lib/systemd/user/code-terminal-mux.service"

  cat > "$deb_root/DEBIAN/control" <<EOF
Package: code-terminal
Version: $version
Section: utils
Priority: optional
Architecture: $deb_arch
Maintainer: Code Terminal Team
Description: Modern terminal emulator
EOF

  deb="$dist_dir/code-terminal_${version}_${deb_arch}.deb"
  dpkg-deb --build "$deb_root" "$deb"
  echo "Pacote .deb gerado: $deb"
else
  echo "dpkg-deb nao encontrado; .deb nao foi gerado. Instale dpkg-dev para gerar pacote Debian/Ubuntu."
fi
