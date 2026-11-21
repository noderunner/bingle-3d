# Builds the OpenXR loader (libopenxr_loader.so.1) that a Linux build with the `vr` feature
# opens at run time, from the OpenXR SDK the openxr-sys crate carries, and copies it and its
# licence into DIR. A release ships it in lib/ beside the binary, which looks there first
# (link with -rpath '$ORIGIN/lib'), so players need no OpenXR package of their own.
#
#   bash scripts/openxr-loader.sh DIR             # for this machine
#   bash scripts/openxr-loader.sh DIR SYSROOT     # for the Steam Frame (arm64), against SYSROOT
#
# Needs CMake, a C++ compiler (clang for the Frame) and the crates fetched (`cargo fetch`).
set -eu
OUT=$1
SYSROOT=${2:-}
mkdir -p "$OUT"
OUT=$(cd "$OUT" && pwd)

# The openxr-sys that Cargo.lock names, wherever Cargo keeps its sources.
V=$(cargo pkgid --locked -p openxr-sys | sed 's/.*[@#]//')
SDK=$(ls -d "${CARGO_HOME:-$HOME/.cargo}"/registry/src/*/openxr-sys-"$V"/OpenXR-SDK | head -1)

if [ -n "$SYSROOT" ]; then BUILD=target/openxr-loader/arm64; else BUILD=target/openxr-loader/host; fi
rm -rf "$BUILD"
mkdir -p "$BUILD"
BUILD=$(cd "$BUILD" && pwd)

ARGS=()
if [ -n "$SYSROOT" ]; then
  SYSROOT=$(cd "$SYSROOT" && pwd)
  cat > "$BUILD/toolchain.cmake" <<EOF
set(CMAKE_SYSTEM_NAME Linux)
set(CMAKE_SYSTEM_PROCESSOR aarch64)
set(CMAKE_SYSROOT $SYSROOT)
set(CMAKE_C_COMPILER clang)
set(CMAKE_CXX_COMPILER clang++)
set(CMAKE_C_COMPILER_TARGET aarch64-linux-gnu)
set(CMAKE_CXX_COMPILER_TARGET aarch64-linux-gnu)
set(CMAKE_FIND_ROOT_PATH_MODE_PROGRAM NEVER)
set(CMAKE_FIND_ROOT_PATH_MODE_LIBRARY ONLY)
set(CMAKE_FIND_ROOT_PATH_MODE_INCLUDE ONLY)
set(CMAKE_FIND_ROOT_PATH_MODE_PACKAGE ONLY)
EOF
  ARGS+=(-DCMAKE_TOOLCHAIN_FILE="$BUILD/toolchain.cmake")
  # The lld Rust ships, so the machine needs none of its own.
  export LDFLAGS="-fuse-ld=lld -B$(rustc --print sysroot)/lib/rustlib/$(rustc -vV | sed -n 's/^host: //p')/bin/gcc-ld"
  export PKG_CONFIG_SYSROOT_DIR=$SYSROOT
  export PKG_CONFIG_LIBDIR=$SYSROOT/usr/lib/aarch64-linux-gnu/pkgconfig:$SYSROOT/usr/share/pkgconfig
fi

# The SDK's samples want a window system's headers (X11) at configure time; only the loader
# is built.
cmake -S "$SDK" -B "$BUILD" "${ARGS[@]}" \
  -DCMAKE_BUILD_TYPE=Release -DDYNAMIC_LOADER=ON \
  -DBUILD_API_LAYERS=OFF -DBUILD_TESTS=OFF -DBUILD_CONFORMANCE_TESTS=OFF -DBUILD_SDK_TESTS=OFF \
  -DBUILD_WITH_SYSTEM_JSONCPP=OFF \
  -DCMAKE_INSTALL_PREFIX="$BUILD/install" -DCMAKE_INSTALL_LIBDIR=lib
cmake --build "$BUILD" -j"$(nproc)"
cmake --install "$BUILD"
cp -L "$BUILD/install/lib/libopenxr_loader.so.1" "$OUT/"
cp "$SDK/LICENSE" "$OUT/LICENSE.openxr-loader"
echo "OpenXR loader in $OUT"
