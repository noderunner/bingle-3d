# Builds a release of bingle-3d for one platform, with the `vr` feature, into
# target/dist/bingle-3d-PLATFORM/: what a player downloads, unpacks and runs, nothing to
# install. The release workflow (.github/workflows/release.yml) archives these folders.
#
#   bash scripts/release-build.sh linux-x86_64    # on Linux: needs CMake and ALSA/udev headers
#   bash scripts/release-build.sh windows-x86_64  # on Windows (Git Bash): needs CMake
#   bash scripts/release-build.sh steam-frame     # on x86-64 Linux, for the Frame: needs clang,
#                                                 # CMake and scripts/frame-sysroot.sh run once
#
# The Frame's folder is also what the SteamOS Devkit Client uploads (README).
set -eu
PLATFORM=$1
OUT=target/dist/bingle-3d-$PLATFORM
rm -rf "$OUT"
mkdir -p "$OUT"
# Symbols stripped at link time: smaller downloads, and no arm64 strip is needed for the Frame.
export CARGO_PROFILE_RELEASE_STRIP=symbols
cargo fetch --locked

case $PLATFORM in
  linux-x86_64)
    # The OpenXR loader is opened at run time; the binary looks for it in lib/ beside itself.
    export RUSTFLAGS="${RUSTFLAGS:-} -C link-arg=-Wl,-rpath,\$ORIGIN/lib"
    cargo build --release --locked --features vr
    cp target/release/bingle-3d "$OUT/"
    bash scripts/openxr-loader.sh "$OUT/lib"
    ;;
  windows-x86_64)
    # The OpenXR loader is linked into the program on Windows.
    cargo build --release --locked --features vr
    cp target/release/bingle-3d.exe "$OUT/"
    ;;
  steam-frame)
    SYS=$(cd target/frame-sysroot/sysroot && pwd)
    T=aarch64-linux-gnu
    GCCLD=$(rustc --print sysroot)/lib/rustlib/$(rustc -vV | sed -n 's/^host: //p')/bin/gcc-ld
    # clang drives the link against the sysroot, with the lld Rust ships.
    export CARGO_TARGET_AARCH64_UNKNOWN_LINUX_GNU_LINKER=clang
    export CARGO_TARGET_AARCH64_UNKNOWN_LINUX_GNU_RUSTFLAGS="-C link-arg=--target=$T -C link-arg=--sysroot=$SYS -C link-arg=-fuse-ld=lld -C link-arg=-B$GCCLD -C link-arg=-Wl,-rpath,\$ORIGIN/lib"
    export CC_aarch64_unknown_linux_gnu=clang
    export CFLAGS_aarch64_unknown_linux_gnu="--target=$T --sysroot=$SYS"
    export AR_aarch64_unknown_linux_gnu=ar
    export PKG_CONFIG_ALLOW_CROSS=1
    export PKG_CONFIG_SYSROOT_DIR=$SYS
    export PKG_CONFIG_LIBDIR=$SYS/usr/lib/$T/pkgconfig:$SYS/usr/share/pkgconfig
    cargo build --release --locked --features vr --target aarch64-unknown-linux-gnu
    cp target/aarch64-unknown-linux-gnu/release/bingle-3d "$OUT/"
    bash scripts/openxr-loader.sh "$OUT/lib" "$SYS"
    ;;
  *)
    echo "unknown platform $PLATFORM: linux-x86_64, windows-x86_64 or steam-frame" >&2
    exit 1
    ;;
esac

cp LICENSE "$OUT/LICENSE"
ls -lR "$OUT"
