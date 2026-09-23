#!/usr/bin/env bash
# Download a private copy of the window library and place it where the
# program expects it. A stripped Windows cannot install the system copy.
set -euo pipefail

dest="src-tauri/webview2"
rm -rf "$dest"
mkdir -p "$dest"

fail() {
  echo "::error title=webview2::$1"
  exit 1
}

take_runtime() {
  local root="$1"
  if [ ! -f "$root/msedgewebview2.exe" ] || [ ! -f "$root/msedge.dll" ]; then
    return 1
  fi
  cp -a "$root"/. "$dest"/
  test -f "$dest/msedgewebview2.exe"
  test -f "$dest/msedge.dll"
}

from_cab() {
  local page="$RUNNER_TEMP/webview2-page.html"
  local cab="$RUNNER_TEMP/webview2.cab"
  local extract_unix
  extract_unix="$(cygpath -u "$RUNNER_TEMP")/webview2-cab"
  curl -fsSL --retry 4 --retry-all-errors --max-time 90 \
    -o "$page" "https://developer.microsoft.com/en-us/microsoft-edge/webview2/" || return 1
  local url
  url=$(tr '"' '\n' < "$page" | sed 's/\\u002[Ff]/\//g; s/&amp;/\&/g' | grep -E 'https?://.*FixedVersionRuntime\.[0-9.]+\.x64\.cab' | head -n 1 || true)
  if [ -z "$url" ]; then
    return 1
  fi
  curl -fL --retry 4 --retry-all-errors --max-time 900 -o "$cab" "$url" || return 1
  rm -rf "$extract_unix"
  mkdir -p "$extract_unix"
  expand.exe '-F:*' "$(cygpath -w "$cab")" "$(cygpath -w "$extract_unix")" || return 1
  local exe root
  exe=$(find "$extract_unix" -iname 'msedgewebview2.exe' | head -n 1 || true)
  if [ -z "$exe" ]; then
    return 1
  fi
  root=$(dirname "$exe")
  take_runtime "$root"
}

from_nuget() {
  local ver="153.0.4234.48"
  local base extract_unix
  base="$(cygpath -u "$RUNNER_TEMP")/webview2-nuget"
  extract_unix="$base/out"
  rm -rf "$base"
  mkdir -p "$extract_unix"
  local id file zip_win out_win
  for id in webview2.runtime.x64 webview2.runtime.x64.core; do
    file="$base/${id}.nupkg"
    curl -fL --retry 4 --retry-all-errors --max-time 900 \
      -o "$file" "https://api.nuget.org/v3-flatcontainer/${id}/${ver}/${id}.${ver}.nupkg" || return 1
    cp -f "$file" "$base/${id}.zip"
    zip_win=$(cygpath -w "$base/${id}.zip")
    out_win=$(cygpath -w "$extract_unix")
    powershell.exe -NoProfile -Command "Expand-Archive -LiteralPath '$zip_win' -DestinationPath '$out_win' -Force" || return 1
  done
  local root
  root=$(find "$extract_unix" -type d -iname 'WebView2' | head -n 1 || true)
  if [ -z "$root" ]; then
    return 1
  fi
  mkdir -p "$dest"
  find "$extract_unix" -type d -iname 'WebView2' -exec cp -a {}/. "$dest"/ \;
  test -f "$dest/msedgewebview2.exe"
  test -f "$dest/msedge.dll"
}

if ! from_cab; then
  rm -rf "$dest"
  mkdir -p "$dest"
  if ! from_nuget; then
    fail "the private window library could not be downloaded"
  fi
fi

if [ ! -f "$dest/msedgewebview2.exe" ] || [ ! -f "$dest/msedge.dll" ]; then
  fail "the private window library is incomplete"
fi
ls -lh "$dest/msedgewebview2.exe" "$dest/msedge.dll"
