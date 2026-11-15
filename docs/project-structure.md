# Project structure

The library implements the tunnel protocol and forwarding behavior. The two
binaries contain example configuration and command-line behavior.

```text
src/
├── lib.rs                       Library entry point and public modules
├── limits.rs                    Shared bounded-queue settings
├── config/                      JSON loading, validation, credentials, codec settings
├── protocol/
│   ├── mod.rs                   Wire actions, frame encoding and decoding
│   └── tests.rs                 Wire compatibility and malformed frames
├── client/
│   ├── mod.rs                   Public client API exports
│   ├── tunnel.rs                Authentication, opening connections, shutdown
│   ├── connection.rs            Connection handles and send/receive halves
│   ├── reconnect/               Scheduled session rotation and failure recovery
│   ├── session/
│   │   ├── mod.rs               Tunnel read/write and dispatch loop
│   │   ├── command.rs           Messages from handles to the driver
│   │   └── driver.rs            Connection state and protocol transitions
│   └── tests/                   Forwarding, failures, and handle lifecycle
├── server/
│   ├── mod.rs                   Public server API exports
│   ├── auth/                    Diesel/MariaDB credentials and durable replay checks
│   ├── fwd_server.rs            Listener, authentication, session startup
│   ├── client_session.rs        Fixed forwarding and multiplexed dispatch
│   ├── connect_policy.rs        Destination resolution and access policy
│   ├── tcp_connection.rs        Fixed TCP destination setup
│   ├── udp_connection.rs        Fixed UDP destination setup
│   ├── endpoints/
│   │   ├── mod.rs               Endpoint tasks, queues, and events
│   │   └── relay.rs             TCP/UDP destination socket relays
│   └── tests/                   TCP, UDP, proxy, and endpoint behavior
└── bin/
    ├── server/
    │   ├── main.rs              Configured server startup
    │   └── options.rs           Server CLI and configuration path
    └── client/
        ├── main.rs              Argument handling, timeouts, exit status
        ├── options.rs           CLI parsing and help text
        ├── config.rs            Configured client patterns and codec construction
        ├── app.rs               One-shot request and listener startup
        ├── tcp_forward.rs       Local TCP listener and bidirectional relay
        ├── tcp_forward/tests.rs TCP listener CLI and relay tests
        ├── transfer.rs          Raw and multiplexed payload transfer
        ├── tests.rs             CLI validation and payload transfer tests
        └── http_proxy/
            ├── mod.rs           Proxy listener and bidirectional relay
            ├── request.rs       Header limits, destinations, request rewriting
            ├── tests.rs         Proxy streaming and failure tests
            └── request/tests.rs HTTP parsing and read-ahead tests
```

`Cargo.toml` keeps the server binary named `bluesky-obfs-tunnel`, so `cargo run`
and `cargo run -- --multiplexed` still start the server. Use
`cargo run --bin client -- ...` for the client.

The default `server` feature includes Diesel and MariaDB authentication. Use
`--no-default-features --bin client` for a client build without database
dependencies, including the [aarch64 Termux build](termux.md).

`cli_pattern.bin` and `serv_pattern.bin` remain at the project root because the
example configuration loads them relative to the working directory.

## Where to put changes

- Add shared wire actions and frame validation in `protocol/`.
- Add settings and validation in `config/`; see [configuration.md](configuration.md).
- Add session rotation and reconnection behavior in `client/reconnect/`.
- Add client API behavior in `client/`; keep socket dispatch in `client/session/`.
- Add destination access rules in `server/connect_policy.rs` and socket relay
  behavior in `server/endpoints/`.
- Add database authentication in `server/auth/` and schema changes in
  `migrations/`; embedded migrations run before the server accepts connections.
- Add command-line options in `bin/client/options.rs` and coordinate them in
  `bin/client/app.rs`.
- Add HTTP parsing behavior in `bin/client/http_proxy/request.rs`.
- Keep tests beside their feature. Test directories use `mod.rs` for shared
  fixtures and separate files for related scenarios.

Shared types are available at `bluesky_obfs_tunnel::StreamLimits` and
`bluesky_obfs_tunnel::protocol`. Existing `server::StreamLimits` and
`server::protocol` imports remain supported through re-exports. Client handles
remain available directly under `bluesky_obfs_tunnel::client`.

## File size and validation

Keep each maintained source file at **300 total lines or fewer**, including
comments and blank lines. Split by responsibility before reaching the limit;
avoid compressing code to fit. Test fixtures and test scenarios follow the same
rule. Generated files such as `Cargo.lock` and build output are excluded.

Run these checks from the repository root:

```sh
python3 scripts/check_file_lengths.py
cargo fmt --all -- --check
cargo test --all-targets
cargo test --doc
```

The size check scans Rust, Python, and shell source files at the root and under
`src/`, `tests/`, `examples/`, `benches/`, and `scripts/`. It exits with an error
and lists every source file exceeding 300 lines.
