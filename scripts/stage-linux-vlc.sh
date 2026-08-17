#!/usr/bin/env bash
# Stage libvlc + plugins + codec libs into src-tauri/vlc-runtime so Linux
# installers (deb / rpm / AppImage) can play video without a system VLC.
set -euo pipefail

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
DEST="$ROOT/src-tauri/vlc-runtime"
ARCH="$(dpkg --print-architecture 2>/dev/null || true)"
case "$(uname -m)" in
  x86_64) TRIPLE="x86_64-linux-gnu"; ARCH="${ARCH:-amd64}" ;;
  aarch64) TRIPLE="aarch64-linux-gnu"; ARCH="${ARCH:-arm64}" ;;
  *)
    echo "stage-linux-vlc: unsupported architecture $(uname -m)" >&2
    exit 1
    ;;
esac

PACKAGES=(
  libvlc5
  libvlccore9
  vlc-plugin-base
  vlc-plugin-video-output
  vlc-data
)

FORCE=0
if [[ "${1:-}" == "--force" ]]; then
  FORCE=1
fi

if [[ "$FORCE" -eq 1 ]]; then
  rm -rf "$DEST"
fi

mkdir -p "$DEST"
if [[ -f "$DEST/libvlc.so.5" && -d "$DEST/plugins" && "$FORCE" -eq 0 ]]; then
  echo "stage-linux-vlc: $DEST already populated"
  exit 0
fi

WORK="$(mktemp -d)"
trap 'rm -rf "$WORK"' EXIT
cd "$WORK"

downloaded=0
if command -v apt-get >/dev/null 2>&1; then
  apt-get download "${PACKAGES[@]}" >/dev/null
  downloaded=1
fi

if [[ "$downloaded" -eq 1 ]]; then
  for deb in ./*.deb; do
    dpkg-deb -x "$deb" extracted
  done
  LIBSRC="extracted/usr/lib/$TRIPLE"
  if [[ ! -e "$LIBSRC/libvlc.so.5" ]]; then
    echo "stage-linux-vlc: extracted debs but libvlc.so.5 was missing" >&2
    exit 1
  fi
else
  LIBSRC="/usr/lib/$TRIPLE"
  if [[ ! -e "$LIBSRC/libvlc.so.5" ]]; then
    echo "stage-linux-vlc: install libvlc5 vlc-plugin-base vlc-plugin-video-output, or run on a machine with apt" >&2
    exit 1
  fi
fi

cp -a "$LIBSRC"/libvlc.so.5* "$DEST/"
cp -a "$LIBSRC"/libvlccore.so.* "$DEST/" 2>/dev/null || true

PLUGIN_SRC=""
for candidate in \
  "$LIBSRC/vlc/plugins" \
  "extracted/usr/lib/vlc/plugins" \
  "/usr/lib/vlc/plugins"
do
  if [[ -d "$candidate" ]]; then
    PLUGIN_SRC="$candidate"
    break
  fi
done
if [[ -z "$PLUGIN_SRC" ]]; then
  echo "stage-linux-vlc: VLC plugins directory not found" >&2
  exit 1
fi
rm -rf "$DEST/plugins"
cp -a "$PLUGIN_SRC" "$DEST/plugins"

# Skip anything the host process already provides. Putting GLib/GTK/OpenSSL on
# LD_LIBRARY_PATH (or next to libvlc without a private rpath) can make WebKitGTK
# load the wrong copy and crash.
is_system_lib() {
  local base
  base="$(basename "$1")"
  case "$base" in
    linux-vdso.so*|ld-linux*.so*|libc.so*|libm.so*|libpthread.so*|libdl.so*|librt.so*|libresolv.so*|libutil.so*|libgcc_s.so*|libstdc++.so*)
      return 0 ;;
    libX11.so*|libXext.so*|libX*.so*|libxcb*.so*|libGL.so*|libEGL.so*|libGLESv2.so*|libGLdispatch.so*|libGLX.so*|libdrm.so*|libgbm.so*|libwayland*.so*|libnvidia*|libcuda*)
      return 0 ;;
    libpulse.so*|libpulsecommon*|libasound.so*|libpipewire*|libdbus-1.so*|libsystemd.so*|libudev.so*)
      return 0 ;;
    libglib-*.so*|libgobject-*.so*|libgio-*.so*|libgmodule-*.so*|libgthread-*.so*|libgtk-*.so*|libgdk-*.so*|libpango*.so*|libcairo*.so*|libatk*.so*|libharfbuzz*.so*|librsvg*.so*|libepoxy.so*)
      return 0 ;;
    libcrypto.so*|libssl.so*|libicu*.so*|libxml2.so*|libfontconfig.so*|libfreetype.so*|libpng*.so*|libjpeg*.so*|libmount.so*|libblkid.so*|libselinux.so*|libpcre*.so*|libffi.so*|libz.so*|libzstd.so*|liblzma.so*|libbz2.so*|libbrotli*.so*)
      return 0 ;;
  esac
  return 1
}

copy_with_links() {
  local src="$1"
  local name
  name="$(basename "$src")"
  [[ -e "$DEST/$name" ]] && return 0
  [[ -e "$src" ]] || return 0
  cp -a "$src" "$DEST/$name"
  if [[ -L "$src" ]]; then
    local real
    real="$(readlink -f "$src")"
    if [[ -n "$real" && -f "$real" ]]; then
      local realname
      realname="$(basename "$real")"
      [[ -e "$DEST/$realname" ]] || cp -a "$real" "$DEST/$realname"
    fi
  fi
}

collect_deps() {
  local file="$1"
  [[ -f "$file" ]] || return 0
  command -v ldd >/dev/null 2>&1 || return 0
  ldd "$file" 2>/dev/null | awk '/=> \// { print $3 }' | while read -r dep; do
    [[ -n "$dep" && -e "$dep" ]] || continue
    if is_system_lib "$dep"; then
      continue
    fi
    copy_with_links "$dep"
  done
}

collect_deps "$DEST/libvlc.so.5"
if [[ -e "$DEST/libvlccore.so.9" ]]; then
  collect_deps "$DEST/libvlccore.so.9"
elif [[ -e "$DEST/libvlccore.so.5" ]]; then
  collect_deps "$DEST/libvlccore.so.5"
fi
find "$DEST/plugins" -name '*.so' -type f | while read -r plugin; do
  collect_deps "$plugin"
done

for _pass in 1 2 3; do
  find "$DEST" -maxdepth 1 \( -name '*.so' -o -name '*.so.*' \) | while read -r lib; do
    [[ -f "$lib" || -L "$lib" ]] || continue
    collect_deps "$lib"
  done
done

# Deb/zip bundlers often drop symlinks. Make every soname a real file so
# `libvlc.so.5` still exists after packaging.
find "$DEST" -maxdepth 1 -type l | while read -r link; do
  real="$(readlink -f "$link")"
  [[ -f "$real" ]] || continue
  rm "$link"
  cp -a "$real" "$link"
done

# Private rpath so libvlc/plugins find bundled codecs without putting this
# directory on the process LD_LIBRARY_PATH (which would shadow GTK).
if command -v patchelf >/dev/null 2>&1; then
  find "$DEST" -maxdepth 1 \( -name '*.so' -o -name '*.so.*' \) -type f | while read -r lib; do
    patchelf --set-rpath '$ORIGIN' "$lib" 2>/dev/null || true
  done
  find "$DEST/plugins" -name '*.so' -type f | while read -r plugin; do
    patchelf --set-rpath '$ORIGIN:$ORIGIN/..' "$plugin" 2>/dev/null || true
  done
fi

if [[ -d extracted/usr/share/doc ]]; then
  mkdir -p "$DEST/licenses"
  while IFS= read -r -d '' copyright; do
    pkg="$(basename "$(dirname "$copyright")")"
    cp "$copyright" "$DEST/licenses/${pkg}.copyright"
  done < <(find extracted/usr/share/doc -name copyright -print0)
fi

echo "stage-linux-vlc: staged $(du -sh "$DEST" | awk '{print $1}') into $DEST"
ls -l "$DEST"/libvlc.so.5 "$DEST"/libvlccore.so.* 2>/dev/null | sed 's|^|  |'
