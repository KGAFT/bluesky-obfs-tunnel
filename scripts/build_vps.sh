#!/usr/bin/env bash
set -euo pipefail

project_dir="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$project_dir"
platform="${1:-linux/amd64}"
if [[ $# -gt 1 || ( "$platform" != linux/amd64 && "$platform" != linux/arm64 ) ]]; then
    echo 'Usage: bash scripts/build_vps.sh [linux/amd64|linux/arm64]' >&2
    exit 1
fi
pattern_dir="${PATTERN_DIR:-$project_dir}"
for pattern in cli_pattern.bin serv_pattern.bin; do
    if [[ ! -s "$pattern_dir/$pattern" ]]; then
        echo "Missing or empty pattern: $pattern_dir/$pattern" >&2
        exit 1
    fi
done

# Build inside Docker so workstation CPU flags and libraries cannot leak in.
docker buildx build --platform "$platform" --load \
    --tag bluesky-obfs-tunnel-server:local .

package_dir="$project_dir/dist/vps"
mkdir -p "$package_dir/config"
docker image save --output "$package_dir/server-image.tar" \
    bluesky-obfs-tunnel-server:local
cp compose.yaml .env.example "$package_dir/"
cp config/server.example.json config/client.example.json "$package_dir/config/"
cp "$pattern_dir/cli_pattern.bin" "$pattern_dir/serv_pattern.bin" "$package_dir/config/"
cp docs/vps-deployment.md "$package_dir/README.md"
(
    cd "$package_dir/config"
    if command -v sha256sum >/dev/null 2>&1; then
        sha256sum cli_pattern.bin serv_pattern.bin > patterns.sha256
    else
        shasum -a 256 cli_pattern.bin serv_pattern.bin > patterns.sha256
    fi
)
# Explicit members keep old runtime configs and secrets out of new bundles.
tar -czf "$project_dir/dist/bluesky-server-vps.tar.gz" -C "$package_dir" \
    server-image.tar compose.yaml .env.example README.md \
    config/server.example.json config/client.example.json \
    config/cli_pattern.bin config/serv_pattern.bin config/patterns.sha256
echo "VPS platform: $platform"
echo "Archive: $project_dir/dist/bluesky-server-vps.tar.gz"
