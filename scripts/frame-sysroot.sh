# Fetches Valve's Steam Runtime 4 arm64 SDK sysroot, the files a build for the Steam Frame links
# against, into target/frame-sysroot/ (or the directory given), checks it against the checksum
# below and makes it usable from outside a container. Run once; scripts/frame-build.sh uses it.
#
#   bash scripts/frame-sysroot.sh [DIR]
#
# The Frame runs native Linux games in "steamrt4 arm64" (the Devkit Client's runtime of that
# name). Valve's older Sniper arm64 runtime was retired before it shipped, so a build against
# it is not what the Frame runs. To move to a newer runtime, change both lines below to a
# release from https://repo.steampowered.com/steamrt4/images/ and its SHA256SUMS.
set -eu
VERSION=4.0.20261110.268882
SHA256=3062a5a92c920e37bdcba9817a2b23a25d7da3f711a3e05ecbb51da091912657
NAME=com.valvesoftware.SteamRuntime.Sdk-arm64-steamrt4-sysroot.tar.gz

DIR=${1:-target/frame-sysroot}
mkdir -p "$DIR"
DIR=$(cd "$DIR" && pwd)
TAR=$DIR/$NAME
if [ -e "$DIR/sysroot/.version" ] && [ "$(cat "$DIR/sysroot/.version")" = "$VERSION" ]; then
  echo "sysroot $VERSION already in $DIR/sysroot"
  exit 0
fi

# About 750 MB. Valve's server sometimes drops HTTP/2 streams, so HTTP/1.1, resumed on failure.
if ! echo "$SHA256  $TAR" | sha256sum -c --status 2>/dev/null; then
  for _ in 1 2 3 4 5; do
    curl -fL --http1.1 -C - -o "$TAR" \
      "https://repo.steampowered.com/steamrt4/images/$VERSION/$NAME" && break
    sleep 5
  done
fi
echo "$SHA256  $TAR" | sha256sum -c

rm -rf "$DIR/sysroot"
mkdir "$DIR/sysroot"
# Its device nodes cannot be made without root, and nothing needs them.
tar -xzf "$TAR" -C "$DIR/sysroot" --no-same-owner --exclude='./dev/*'

# Its links are absolute (/lib/...), which only works as a chroot: point them into the
# sysroot, so a linker outside it finds the shared libraries rather than the static ones.
SYS=$DIR/sysroot
find "$SYS" -type l -lname '/*' ! -lname "$SYS/*" -print0 |
  while IFS= read -r -d '' l; do ln -sfn "$SYS$(readlink "$l")" "$l"; done

echo "$VERSION" > "$SYS/.version"
echo "sysroot $VERSION in $SYS"
