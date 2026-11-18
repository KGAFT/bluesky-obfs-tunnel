# aarch64 Android client for Termux

The client can be cross-compiled on Linux or macOS for
`aarch64-linux-android`. The build script selects Android API 24 (Android 7.0),
uses a generic ARM64 CPU target, and requests 16 KiB ELF page alignment. It
builds only the client with `--no-default-features`; Diesel and MariaDB remain
on the server. The server feature stays enabled by default for normal builds.

## Build on your computer

Install Rust, CMake, Perl, Git, and libclang (needed by bindgen). Download and
extract the [Android NDK](https://developer.android.com/ndk/downloads/), using
the package for your build computer. The build was compiled with NDK r30.
Rust documents the target in its
[Android platform support guide](https://doc.rust-lang.org/nightly/rustc/platform-support/android.html).

From the repository root:

```sh
rustup target add aarch64-linux-android
export ANDROID_NDK_HOME=/path/to/android-ndk-r30
bash scripts/build_termux_aarch64.sh
```

Outputs:

- `dist/termux-aarch64/client`: the ARM64 Android executable.
- `dist/termux-aarch64/cli_pattern.bin` and `serv_pattern.bin`: handshake patterns.
- `dist/termux-aarch64/client.example.json`: client settings to customize.
- `dist/termux-aarch64/patterns.sha256`: checksums for the bundled pair.
- `dist/bluesky-client-termux-aarch64.tar.gz`: the complete directory to transfer.

The script isolates host Cargo flags and explicitly selects a generic ARM64 CPU.
It links BoringSSL and the C++ runtime statically; the resulting executable still
uses Android's system C library and dynamic linker. No MariaDB, OpenSSL, or C++
shared library needs to be copied to the phone.

The default package uses the shipped Chrome 149/Microsoft pattern pair. To
package a server's captured pair, retrieve both files into one directory and run
`PATTERN_DIR=/path/to/server-patterns bash scripts/build_termux_aarch64.sh`.
Match that server's cover settings and browser profile in the phone's JSON.
Rebuild the archive after changing the pair; an old archive keeps its old files.

## Run in Termux

Transfer the archive to the phone's Download directory. In Termux, allow shared
storage access if it has not already been granted, then extract into Termux's
home directory:

```sh
termux-setup-storage
cd "$HOME"
tar -xzf ~/storage/downloads/bluesky-client-termux-aarch64.tar.gz
cd termux-aarch64
chmod +x client
cp client.example.json client.json
chmod 600 client.json
sha256sum -c patterns.sha256
./client --help
```

Run the executable from Termux's private home directory. Android shared storage,
including Download and `/sdcard`, is mounted with execution disabled; see the
[Termux execution environment documentation](https://github.com/termux/termux-packages/wiki/Termux-execution-environment).

Edit `client.json`:

- Set `server_address` to your server's reachable address and published port.
- Set `client_identity` and `client_password` to a client created in MariaDB
  with the server's `--create-client` command.
- Match the server's public password, server ID, cover settings, and patterns.

Keep the pattern files next to the executable and run it from that directory;
their default paths are relative to the current working directory. Start a
local HTTP/HTTPS proxy against a multiplexed tunnel server:

```sh
./client --config client.json --http-proxy 127.0.0.1:8080
```

Or expose a TCP service reachable from the server:

```sh
./client --config client.json --tcp 127.0.0.1:2080:127.0.0.1:1080
```

The `--tcp` destination is resolved from the server's network, so its loopback
address refers to the server. These commands run a local proxy or forwarder;
configure the application that should use that listener. Android execution
and network behavior have not been tested on a phone as part of this build.

See [database setup](database.md) in the repository for creating client identities.
