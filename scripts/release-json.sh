# Prints a release's release.json: its version, and the version of Bingle's API its builds
# were made for. A server that offers the client's builds (Bingle's docs/client-builds.md)
# keeps the file in the version's folder with the archives, and offers the builds only when
# its own API can be used by them.
#
#   bash scripts/release-json.sh > target/dist/release.json
#
# The version is Cargo.toml's; the API is API_VERSION in src/net/api.rs, where a test holds it
# to the form read here.
set -eu
version=$(sed -n 's/^version = "\(.*\)"/\1/p' Cargo.toml | head -1)
api=$(sed -n '/^pub const API_VERSION/,/^};/p' src/net/api.rs |
  sed -n 's/^ *\(major\|minor\|patch\): \([0-9]*\),.*/\2/p' | paste -sd. -)
case "$version/$api" in
  [0-9]*.[0-9]*.[0-9]*/[0-9]*.[0-9]*.[0-9]*) ;;
  *)
    echo "could not read the version ($version) or the API ($api)" >&2
    exit 1
    ;;
esac
printf '{"version": "%s", "api": "%s"}\n' "$version" "$api"
