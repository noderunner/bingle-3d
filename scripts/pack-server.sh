# Runs a Bingle server with an install pack whose look this client fetches (Bingle's ADR 0147):
# Bingle's Draughts fixture pack, marked private, with tests/pack/clients/bingle-3d/ as its
# client folder. Bingle is built from origin/main of the checkout beside this one, into a
# scratch folder, without touching that checkout. Needs Go.
#
#   bash scripts/pack-server.sh [scratch folder]     # then, in another shell:
#   BINGLE3D_CONFIG_DIR=$(mktemp -d) BINGLE3D_CACHE_DIR=$(mktemp -d) \
#     ./target/debug/bingle-3d --server http://127.0.0.1:18080
set -euo pipefail
here=$(cd "$(dirname "$0")/.." && pwd)
bingle=${BINGLE_CHECKOUT:-$here/../bingle}
work=${1:-$(mktemp -d)}
mkdir -p "$work/src" "$work/packs"
git -C "$bingle" archive origin/main | tar -x -C "$work/src"
(cd "$work/src" && go build -o "$work/bingle" .)
pack="$work/packs/draughts"
rm -rf "$pack"
cp -R "$work/src/tests/testdata/draughts/pack" "$pack"
echo "private: true" >> "$pack/manifest.yaml"
(cd "$work/src" && GOOS=wasip1 GOARCH=wasm CGO_ENABLED=0 go build -buildmode=c-shared -trimpath \
  -o "$pack/rules.wasm" ./tests/testdata/draughts/wasm)
mkdir -p "$pack/clients"
cp -R "$here/tests/pack/clients/bingle-3d" "$pack/clients/"
echo "serving the pack from $work on http://127.0.0.1:18080" >&2
BINGLE_ADDR=127.0.0.1:18080 BINGLE_METRICS_ADDR=127.0.0.1:18081 BINGLE_DB_PATH="$work/bingle.db" \
  BINGLE_PACKS="$work/packs" exec "$work/bingle"
