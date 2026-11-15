#!/usr/bin/env bash
set -euo pipefail

project_dir="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$project_dir"
: "${ANDROID_NDK_HOME:?Set ANDROID_NDK_HOME to an extracted Android NDK}"
export ANDROID_NDK_HOME
pattern_dir="${PATTERN_DIR:-$project_dir}"
for pattern in cli_pattern.bin serv_pattern.bin; do
    if [[ ! -s "$pattern_dir/$pattern" ]]; then
        echo "Missing or empty pattern: $pattern_dir/$pattern" >&2
        exit 1
    fi
done

case "$(uname -s)" in
    Linux) ndk_host=linux-x86_64 ;;
    Darwin) ndk_host=darwin-x86_64 ;;
    *) echo 'Build from Linux or macOS using the matching Android NDK.' >&2; exit 1 ;;
esac
ndk_bin="$ANDROID_NDK_HOME/toolchains/llvm/prebuilt/$ndk_host/bin"
if [[ ! -x "$ndk_bin/aarch64-linux-android24-clang" ]]; then
    echo "Android API 24 compiler not found under $ndk_bin" >&2
    exit 1
fi

export CARGO_TARGET_AARCH64_LINUX_ANDROID_LINKER="$ndk_bin/aarch64-linux-android24-clang"
export CC_aarch64_linux_android="$ndk_bin/aarch64-linux-android24-clang"
export CXX_aarch64_linux_android="$ndk_bin/aarch64-linux-android24-clang++"
export AR_aarch64_linux_android="$ndk_bin/llvm-ar"
export RANLIB_aarch64_linux_android="$ndk_bin/llvm-ranlib"
export CMAKE_TOOLCHAIN_FILE_aarch64_linux_android="$project_dir/scripts/android-aarch64.cmake"
export BINDGEN_EXTRA_CLANG_ARGS_aarch64_linux_android=--target=aarch64-linux-android24
# Bundle the C++ runtime used by BoringSSL, so no Termux C++ package is needed.
export BORING_BSSL_RUST_CPPLIB_aarch64_linux_android=static=c++_static
# Expose only the C++ archives to rustc. Adding the whole NDK library directory
# would put its static libc ahead of the API-specific Android shared libraries.
target_dir="${CARGO_TARGET_DIR:-target}"
runtime_dir="$target_dir/termux-aarch64-runtime"
mkdir -p "$runtime_dir"
runtime_dir="$(cd "$runtime_dir" && pwd)"
ndk_lib="$ndk_bin/../sysroot/usr/lib/aarch64-linux-android"
cp "$ndk_lib/libc++_static.a" "$ndk_lib/libc++abi.a" "$runtime_dir/"
# Isolate host Cargo flags and request compatibility with 16 KiB pages.
rust_flags=(
    -C target-cpu=generic
    -L "native=$runtime_dir"
    -C link-arg=-lc++abi
    -C link-arg=-Wl,-z,max-page-size=16384
)
printf -v CARGO_ENCODED_RUSTFLAGS '%s\x1f' "${rust_flags[@]}"
export CARGO_ENCODED_RUSTFLAGS="${CARGO_ENCODED_RUSTFLAGS%$'\x1f'}"

cargo build --locked --release --no-default-features --bin client \
    --target aarch64-linux-android

package_dir="$project_dir/dist/termux-aarch64"
mkdir -p "$package_dir"
cp "$target_dir/aarch64-linux-android/release/client" "$package_dir/client"
cp "$pattern_dir/cli_pattern.bin" "$pattern_dir/serv_pattern.bin" "$package_dir/"
cp config/client.example.json "$package_dir/client.example.json"
cp docs/termux.md "$package_dir/README.md"
cp docs/database.md "$package_dir/database.md"
(
    cd "$package_dir"
    if command -v sha256sum >/dev/null 2>&1; then
        sha256sum cli_pattern.bin serv_pattern.bin > patterns.sha256
    else
        shasum -a 256 cli_pattern.bin serv_pattern.bin > patterns.sha256
    fi
)
tar -czf "$project_dir/dist/bluesky-client-termux-aarch64.tar.gz" \
    -C "$project_dir/dist" \
    termux-aarch64/client termux-aarch64/cli_pattern.bin \
    termux-aarch64/serv_pattern.bin termux-aarch64/client.example.json \
    termux-aarch64/README.md termux-aarch64/database.md termux-aarch64/patterns.sha256
echo "Termux files: $package_dir"
echo "Archive: $project_dir/dist/bluesky-client-termux-aarch64.tar.gz"
