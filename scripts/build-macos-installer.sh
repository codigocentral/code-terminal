#!/usr/bin/env bash
set -euo pipefail

repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$repo_root"

mode="${1:-local}"
if [[ "$#" -gt 1 || ( "$mode" != local && "$mode" != --app-store && "$mode" != --developer-id ) ]]; then
  echo "Uso: $0 [--app-store | --developer-id]" >&2
  exit 2
fi

version="$(sed -nE 's/^version[[:space:]]*=[[:space:]]*"([^"]+)".*/\1/p' Cargo.toml | head -n 1)"
if [[ -z "$version" ]]; then
  echo "Nao foi possivel ler a versao do Cargo.toml." >&2
  exit 1
fi

arch="$(uname -m)"
bundle_id="com.codeterm.codeterminal"
app_name="Code Terminal"
app_bundle="$app_name.app"
dist_dir="$repo_root/dist"
package_root="$repo_root/target/package/macos"
build_number="$version"
if [[ "$mode" == --developer-id ]]; then
  package_root="$package_root-developer-id"
  : "${MACOS_APP_SIGN_IDENTITY:?Informe a identidade Developer ID Application.}"
  : "${MACOS_NOTARY_PROFILE:?Informe o perfil local do notarytool.}"
  identities="$(security find-identity -v -p codesigning)"
  if ! grep -F -- "$MACOS_APP_SIGN_IDENTITY" <<< "$identities" | grep -Fq '"Developer ID Application:'; then
    echo "Identidade Developer ID Application indisponível no Chaves." >&2
    exit 1
  fi
fi
if [[ "$mode" == --app-store ]]; then
  package_root="$package_root-app-store"
  build_number="${MACOS_BUILD_NUMBER:-1}"
  if [[ ! "$build_number" =~ ^[1-9][0-9]{0,3}(\.[0-9]{1,2}){0,2}$ ]]; then
    echo "Defina MACOS_BUILD_NUMBER com um CFBundleVersion válido, por exemplo 1." >&2
    exit 2
  fi
  : "${MACOS_APP_SIGN_IDENTITY:?Informe a identidade Apple Distribution do app.}"
  : "${MACOS_INSTALLER_SIGN_IDENTITY:?Informe a identidade Mac Installer Distribution do pkg.}"
  : "${MACOS_APP_STORE_PROFILE:?Informe o caminho do perfil Mac App Store Connect.}"

  app_identities="$(security find-identity -v -p codesigning)"
  installer_identities="$(security find-identity -v)"
  app_identity_hash="$(awk -F '"' -v identity="$MACOS_APP_SIGN_IDENTITY" '
    $2 ~ /^Apple Distribution:/ {
      split($1, fields, " ")
      if ($2 == identity || fields[2] == identity) print fields[2]
    }
  ' <<< "$app_identities")"
  if [[ ! "$app_identity_hash" =~ ^[A-Fa-f0-9]{40}$ ]]; then
    echo "Use o nome completo ou o hash de uma identidade Apple Distribution válida." >&2
    exit 1
  fi
  if ! grep -F -- "$MACOS_INSTALLER_SIGN_IDENTITY" <<< "$installer_identities" | grep -Eq '"(3rd Party Mac Developer Installer|Mac Installer Distribution):'; then
    echo "Identidade Mac Installer Distribution indisponível. Consulte security find-identity -v." >&2
    exit 1
  fi
  profile_plist="$(security cms -D -i "$MACOS_APP_STORE_PROFILE")"
  profile_app_id="$(plutil -extract Entitlements.com\\.apple\\.application-identifier raw -o - - <<< "$profile_plist")"
  profile_team="$(plutil -extract TeamIdentifier.0 raw -o - - <<< "$profile_plist")"
  if [[ "$profile_app_id" != "$profile_team.$bundle_id" ]]; then
    echo "O perfil precisa ser do App ID $bundle_id." >&2
    exit 1
  fi
  profile_cert_hash="$(plutil -extract DeveloperCertificates.0 raw -o - - <<< "$profile_plist" |
    openssl base64 -d -A | shasum | awk '{ print toupper($1) }')"
  if [[ "$profile_cert_hash" != "$(tr '[:lower:]' '[:upper:]' <<< "$app_identity_hash")" ]]; then
    echo "O certificado Apple Distribution selecionado precisa constar no perfil." >&2
    exit 1
  fi
fi

stage_dir="$package_root/stage"
app_dir="$stage_dir/Applications/$app_bundle"
resources="$app_dir/Contents/Resources"
executables="$app_dir/Contents/MacOS"

# -j 1 por padrão por causa do consumo de memória do LTO.
build_args=(--release --bins -j "${CARGO_BUILD_JOBS:-1}")
if [[ "$mode" == --developer-id ]]; then
  build_args+=(--features macos-notarized)
fi
cargo build "${build_args[@]}"
release_dir="${CARGO_TARGET_DIR:-target}/release"

rm -rf "$package_root"
mkdir -p "$dist_dir" "$executables" "$resources"

install -m 755 "$release_dir/code-terminal" "$executables/code-terminal"
install -m 755 "$release_dir/code-terminal-browser" "$executables/code-terminal-browser"
if [[ "$mode" == --app-store ]]; then
  install -m 755 "$release_dir/code-terminal-cli" "$executables/code-terminal-cli"
  cp "$MACOS_APP_STORE_PROFILE" "$app_dir/Contents/embedded.provisionprofile"
else
  mkdir -p "$resources/bin" "$stage_dir/usr/local/bin"
  for helper in code-terminal-cli code-terminal-browser; do
    install -m 755 "$release_dir/$helper" "$resources/bin/$helper"
    install -m 755 "$release_dir/$helper" "$stage_dir/usr/local/bin/$helper"
  done
fi

cp packaging/macos/Credits.rtf LICENSE LICENSE-PLUGIN-EXCEPTION.md THIRD_PARTY_NOTICES.md "$resources/"
iconset="$package_root/CodeTerminal.iconset"
mkdir -p "$iconset"
for size in 16 32 128 256 512; do
  sips -z "$size" "$size" assets/brand/code-terminal.png --out "$iconset/icon_${size}x${size}.png" >/dev/null
  double=$((size * 2))
  sips -z "$double" "$double" assets/brand/code-terminal.png --out "$iconset/icon_${size}x${size}@2x.png" >/dev/null
done
iconutil -c icns "$iconset" -o "$resources/CodeTerminal.icns"

cat > "$app_dir/Contents/Info.plist" <<EOF
<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
  <key>CFBundleDevelopmentRegion</key>
  <string>en</string>
  <key>CFBundleExecutable</key>
  <string>code-terminal</string>
  <key>CFBundleIdentifier</key>
  <string>$bundle_id</string>
  <key>CFBundleIconFile</key>
  <string>CodeTerminal.icns</string>
  <key>LSApplicationCategoryType</key>
  <string>public.app-category.developer-tools</string>
  <key>NSHumanReadableCopyright</key>
  <string>© 2026 Código Central Suporte em Informática Ltda.</string>
  <key>CFBundleName</key>
  <string>$app_name</string>
  <key>CFBundlePackageType</key>
  <string>APPL</string>
  <key>CFBundleShortVersionString</key>
  <string>$version</string>
  <key>CFBundleVersion</key>
  <string>$build_number</string>
  <key>LSMinimumSystemVersion</key>
  <string>12.0</string>
  <key>NSHighResolutionCapable</key>
  <true/>
</dict>
</plist>
EOF

if [[ "$mode" == --app-store ]]; then
  app_entitlements="$package_root/app-store.entitlements"
  cp packaging/macos/app-store.entitlements "$app_entitlements"
  /usr/libexec/PlistBuddy -c "Add :com.apple.application-identifier string $profile_app_id" "$app_entitlements"
  /usr/libexec/PlistBuddy -c "Add :com.apple.developer.team-identifier string $profile_team" "$app_entitlements"

  for helper in code-terminal-cli code-terminal-browser; do
    codesign --force --sign "$MACOS_APP_SIGN_IDENTITY" \
      --entitlements packaging/macos/app-store-helper.entitlements "$executables/$helper"
  done
  codesign --force --sign "$MACOS_APP_SIGN_IDENTITY" --entitlements "$app_entitlements" "$app_dir"
  codesign --verify --deep --strict --verbose=2 "$app_dir"
  signature="$(codesign --display --verbose=4 "$app_dir" 2>&1)"
  if ! grep -Fxq "TeamIdentifier=$profile_team" <<< "$signature"; then
    echo "A identidade Apple Distribution e o perfil precisam pertencer ao mesmo time." >&2
    exit 1
  fi

  store_dir="$(mktemp -d "$dist_dir/.mac-app-store.XXXXXXXX")"
  trap 'rm -rf "$store_dir"' EXIT
  pkg_name="CodeTerminal-$version-mac-app-store-$arch-$build_number.pkg"
  productbuild --sign "$MACOS_INSTALLER_SIGN_IDENTITY" \
    --component "$app_dir" /Applications "$store_dir/$pkg_name"
  pkgutil --check-signature "$store_dir/$pkg_name"
  mv -f "$store_dir/$pkg_name" "$dist_dir/$pkg_name"
  echo "Pacote Mac App Store gerado: $dist_dir/$pkg_name"
  exit 0
fi

if [[ "$mode" == --developer-id ]]; then
  for helper in "$executables/code-terminal-browser" "$resources/bin/"*; do
    codesign --force --timestamp --options runtime --sign "$MACOS_APP_SIGN_IDENTITY" "$helper"
  done
  codesign --force --timestamp --options runtime --sign "$MACOS_APP_SIGN_IDENTITY" "$app_dir"
  codesign --verify --deep --strict --verbose=2 "$app_dir"

  dmg_root="$package_root/dmg"
  mkdir -p "$dmg_root"
  ditto "$app_dir" "$dmg_root/$app_bundle"
  ln -s /Applications "$dmg_root/Applications"
  dmg_name="CodeTerminal-$version-macos-$arch-notarized.dmg"
  output_dir="$(mktemp -d "$dist_dir/.macos-notary.XXXXXXXX")"
  trap 'rm -rf "$output_dir"' EXIT
  dmg="$output_dir/$dmg_name"
  hdiutil create -volname "Code Terminal $version" -srcfolder "$dmg_root" -format UDZO "$dmg"
  codesign --timestamp --sign "$MACOS_APP_SIGN_IDENTITY" "$dmg"
  xcrun notarytool submit "$dmg" --keychain-profile "$MACOS_NOTARY_PROFILE" --wait --timeout 20m
  xcrun stapler staple "$dmg"
  xcrun stapler validate "$dmg"
  spctl --assess --type open --context context:primary-signature --verbose=2 "$dmg"
  mv -f "$dmg" "$dist_dir/$dmg_name"
  echo "DMG notarizado: $dist_dir/$dmg_name"
  exit 0
fi

if command -v codesign >/dev/null 2>&1; then
  for helper in "$executables/code-terminal-browser" "$resources/bin/"* "$stage_dir/usr/local/bin/"*; do
    codesign --force --sign - "$helper"
  done
  codesign --force --sign - "$app_dir"
fi

tarball="$dist_dir/CodeTerminal-$version-macos-$arch-app.tar.gz"
tar -C "$stage_dir/Applications" -czf "$tarball" "$app_bundle"
echo "App tar.gz gerado: $tarball"

if command -v pkgbuild >/dev/null 2>&1; then
  pkg="$dist_dir/CodeTerminal-$version-macos-$arch.pkg"
  pkgbuild \
    --root "$stage_dir" \
    --identifier "$bundle_id" \
    --version "$version" \
    --install-location "/" \
    "$pkg"
  echo "Pacote .pkg gerado: $pkg"
else
  echo "pkgbuild nao encontrado; .pkg nao foi gerado. Instale Xcode Command Line Tools."
fi

if command -v hdiutil >/dev/null 2>&1; then
  dmg_root="$package_root/dmg"
  dmg="$dist_dir/CodeTerminal-$version-macos-$arch.dmg"
  mkdir -p "$dmg_root"
  cp -R "$app_dir" "$dmg_root/"
  # "Arraste para instalar": o atalho para /Applications ao lado do app (sem
  # ele, quem abre o app de dentro do .dmg roda da imagem montada e depois
  # não acha o programa).
  ln -s /Applications "$dmg_root/Applications"
  rm -f "$dmg"
  hdiutil create -volname "Code Terminal $version" -srcfolder "$dmg_root" -ov -format UDZO "$dmg"
  echo "DMG gerado: $dmg"
else
  echo "hdiutil nao encontrado; .dmg nao foi gerado. Execute em macOS."
fi
