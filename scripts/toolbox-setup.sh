# Sets up a toolbox container for building bingle-3d on an immutable Fedora desktop (Bazzite,
# Silverblue, Kinoite). Run it on the host with `bash scripts/toolbox-setup.sh`; it is safe to
# run again.
#
# A toolbox is a podman container that shares your home directory, user, display and devices, so
# the packages below go into the container and the system image is left alone. Build inside it
# (`toolbox run -c bingle-3d cargo build`) and run the binary from either side. With NVIDIA's
# driver, run it on the host (`./target/debug/bingle-3d`): the container has no NVIDIA userspace
# and renders in software.
set -euo pipefail

NAME=${BINGLE_TOOLBOX:-bingle-3d}
RELEASE=${BINGLE_TOOLBOX_RELEASE:-44}

if ! toolbox list --containers | awk 'NR > 1 { print $2 }' | grep -qx "$NAME"; then
    toolbox create -y --distro fedora --release "$RELEASE" "$NAME"
fi

# What Bevy links against on Linux (audio, gamepads, Wayland and X11 windows), a C toolchain for
# the -sys crates, Vulkan with Mesa's drivers (including the software one, lavapipe), and Xvfb for
# running headless as CI does.
toolbox run -c "$NAME" sudo dnf install -y \
    gcc gcc-c++ clang pkgconf-pkg-config \
    alsa-lib-devel systemd-devel \
    wayland-devel libxkbcommon-devel libxkbcommon-x11-devel \
    libX11-devel libXcursor-devel libXi-devel libXrandr-devel \
    vulkan-loader vulkan-tools mesa-vulkan-drivers mesa-dri-drivers \
    xorg-x11-server-Xvfb \
    rustup

# rustup lives in ~/.cargo and ~/.rustup, shared with the host. rust-toolchain.toml picks the
# compiler version; this just makes sure rustup itself is set up.
if [ ! -x "$HOME/.cargo/bin/rustup" ]; then
    toolbox run -c "$NAME" rustup-init -y --profile minimal --default-toolchain none --no-modify-path
fi

echo
echo "Ready. Build with:  toolbox run -c $NAME bash -lc 'cargo build'"
echo "(add ~/.cargo/bin to PATH in your shell profile if cargo is not found)"
