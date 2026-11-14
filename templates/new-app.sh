#!/usr/bin/env bash
# Create a new Android app from an oxydroid starter template.
# Usage: templates/new-app.sh <egui|bevy> <app-name> [target-dir]   (default: ./<app-name>)
#   --git   depend on oxydroid from GitHub instead of this checkout (for apps outside the repo
#           that will be shared or built elsewhere)
# Then:  cd <app-name> && cargo apk run --lib     (needs the Android SDK/NDK and cargo-apk)
set -euo pipefail
git_deps=0
args=()
for a in "$@"; do [[ "$a" == --git ]] && git_deps=1 || args+=("$a"); done
[[ ${#args[@]} -ge 2 ]] || { sed -n '2,7p' "$0"; exit 2; }
kind=${args[0]}; name=${args[1]}; dest=${args[2]:-$name}
here="$(cd "$(dirname "$0")" && pwd)"; repo="$(cd "$here/.." && pwd)"
src="$here/$kind-app"
[[ -d "$src" ]] || { echo "unknown template '$kind' (egui or bevy)"; exit 2; }
[[ "$name" =~ ^[a-z][a-z0-9_-]*$ ]] || { echo "app name: lowercase letters, digits, - or _"; exit 2; }
[[ -e "$dest" ]] && { echo "$dest already exists"; exit 3; }

mkdir -p "$dest"
cp -r "$src/src" "$src/Cargo.toml" "$dest/"
ident=$(tr '-' '_' <<<"$name")
pkg="dev.example.$(tr -d '_-' <<<"$name")"   # change to your own reverse-domain id before release
sed -i -e "s|^name = \"oxydroid-$kind-app\"|name = \"$name\"|" \
       -e "s|^package = \".*\"|package = \"$pkg\"|" \
       -e "s|^label = \".*\"|label = \"$name\"|" \
       -e "s|^# Make your own copy with:.*|# Created from the oxydroid $kind starter.|" "$dest/Cargo.toml"
if [[ $git_deps == 1 ]]; then
  url=https://github.com/abdorah/oxydroid
  sed -i -e "s|oxydroid = { path = \"../..\"|oxydroid = { git = \"$url\"|" \
         -e "s|oxydroid-bevy = { path = \"../../bevy\"|oxydroid-bevy = { git = \"$url\"|" "$dest/Cargo.toml"
else
  sed -i -e "s|path = \"../../bevy\"|path = \"$repo/bevy\"|" \
         -e "s|path = \"../..\"|path = \"$repo\"|" "$dest/Cargo.toml"
fi
printf 'target/\nCargo.lock\n' > "$dest/.gitignore"
echo "Created $dest ($kind starter, package $pkg, crate $ident)."
echo "Next: cd $dest && cargo apk run --lib"
