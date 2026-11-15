# Configuration and reconnecting

Both binaries read validated JSON settings. Without `--config`, the server uses
`server.json` and the client uses `client.json` in the working directory. Missing
files are created with defaults; an existing malformed or invalid file stops
startup with an error. Omitted fields use defaults. Unknown fields, including
TUN settings, are rejected at every configuration level.

Start the supplied multiplexed server and proxy client in separate terminals:

```sh
cargo run -- --config config/server.example.json
cargo run --bin client -- --config config/client.example.json --proxy
```

The complete settings are in [server.example.json](../config/server.example.json)
and [client.example.json](../config/client.example.json). Copy them before
customizing them. Both `--config PATH` and `--config=PATH` are supported.

The server listens on `0.0.0.0:443` by default; the client's default server is
`127.0.0.1:443`. Existing JSON files retain their configured addresses.
The default server settings preserve fixed TCP forwarding to `127.0.0.1:2080`.
The supplied server example selects multiplexed forwarding for the HTTP proxy.
`--multiplexed` overrides the server's configured mode. Server `--bind IP:PORT`
overrides `bind_address`, and `--tcp IP:PORT` selects a fixed TCP destination.
Server `--tcp` and `--multiplexed` cannot be combined. Client flags `--server`,
`--http-proxy`, `--timeout`, and `--setup-port` override file values in memory.
Overrides are not saved back to the file. `--proxy` uses `local_proxy_bind`.
`--help` prints usage without creating or loading a configuration file.

For a persistent local TCP forward, use
`client --server HOST:443 --listen 127.0.0.1:1080 --tcp 127.0.0.1:2080`
against a server started with `--multiplexed`. `--listen` is a CLI-only address;
the destination supplied to `--tcp` is reached from the server. The client
relays bytes unchanged, with no HTTP parsing or SOCKS negotiation of its own.
It accepts concurrent connections and preserves TCP half-closes. `--listen`
requires `--tcp` and rejects `--udp`, proxy, and payload options.

## Client session lifetime

The client's `reconnect` section controls long-running proxy and TCP listener sessions:

```json
{
  "reconnect": {
    "enabled": true,
    "session_min_secs": 1800,
    "session_max_secs": 2400,
    "retry_delay_secs": 5
  }
}
```

After each successful authentication, a fresh random lifetime is chosen from
the **inclusive** interval, so these defaults rotate after 30–40 minutes. Equal
bounds give a fixed lifetime. Set `enabled` to `false` to disable scheduled
rotation. Bounds and retry delays must remain positive, with maximum at least
minimum.

At the deadline, the old tunnel and its active streams close.
The client immediately authenticates a new tunnel. Existing TCP connections
cannot migrate between tunnels; applications must open new connections. The local
listener stays bound to the same address throughout. Failed handshakes,
setup timeouts, and dropped tunnels retry after `retry_delay_secs`, including
when scheduled rotation is disabled. Ctrl-C cancels the listener, its connections,
the active tunnel, and any pending reconnect attempt.

One-shot `--tcp`, `--udp`, and fixed TCP payload requests retain their request
timeout and return their result once. They do not retry or resend application
data. Library users can opt into the same lifecycle with
`client::reconnect::supervise` and their own connection/service callbacks.

## Settings

| Section / field | Purpose |
| --- | --- |
| `obfuscation` | Cover SNI/destination, browser profile, public password, server ID, padding, delays, handshake timeout, local setup proxy port |
| `patterns` | Client/server pattern paths and server capture destination/URL |
| `queues` | Endpoint event, per-stream pending frame, and tunnel writer queue capacities |
| Server `database` | MariaDB URL environment variable, pool size, and connection acquisition timeout; see [database setup](database.md) |
| Server `forwarding` | Fixed TCP/UDP destination, legacy HTTP proxy protocol, or multiplexed TCP/UDP |
| Server `allow_local_proxy_connections` | Permit local destinations; defaults to `true` for the existing local examples |
| Server `local_proxy_allowlist` | Specific `host:port` targets allowed when local destinations are otherwise denied |
| Server `public_ips` | This host's own public IPs to block when local connections are denied |
| Server `rate_limiter` | Three receive-pattern deviation fractions from 0 to 1; `null` disables it |
| Server `replay_guard_window_ms` | Positive accepted handshake clock skew and nonce retention in milliseconds; defaults to 60000 |
| Client `server_address` | Tunnel host and port; DNS names and bracketed IPv6 addresses are supported |
| Client `local_proxy_bind` | Local HTTP/HTTPS proxy listener address |
| Client `client_identity`, `client_password` | Credentials created in MariaDB with the server's `--create-client` command |
| Client `proxy_setup_timeout_secs` | Tunnel setup, request headers, destination open, and one-shot request timeout |
| Client `proxy_setup_queue_depth` | Maximum concurrent accepted proxy connections |

Server forwarding values are `{"mode":"multiplexed"}`, `{"mode":"http_proxy"}`,
`{"mode":"tcp","destination":"127.0.0.1:2080"}`, or
`{"mode":"udp","destination":"127.0.0.1:5353"}`. Fixed destinations are literal
IP addresses; multiplexed destinations can be DNS names.

Both peers must agree on credentials, public password, server ID, cover site,
and captured patterns. Browser profiles serialize as names such as
`chrome_149` (the default) or `chrome_141`. Delay values use
`{"ArraySort":{"len":13,"repeats":1}}` or `{"SpinLoop":{"micros":55}}`.
Padding and long-delay upper bounds are exclusive, matching `FakeCodecCfg`.

Relative pattern paths are resolved from the **working directory**, as before.
The client only loads existing files. The server can capture missing patterns
from the configured cover site and creates parent directories for them.
The shipped pair uses Chrome 149 and `https://www.microsoft.com/`, matching
the defaults. Changing the capture settings does not recapture an existing valid
pattern pair. Replace both files together when switching cover site or browser;
restart the server and distribute its actual pair to clients. For Compose, the
working directory is `/app/config`, backed by the host `config/` directory.

The examples retain the original demonstration passwords. Configure your own
values for deployment. Newly created settings files have mode `0600` on Unix.
Runtime `client.json` and `server.json` at the root or under `config/` are ignored
by Git. Deployment examples are templates: copy `server.example.json` to
`config/server.json` and set `SERVER_CONFIG` accordingly. Existing files, including
old copies in `target/release/`, are never migrated automatically. Remove obsolete
server `clients` arrays and create identities in MariaDB before using the new
server. See the [deployment update checklist](vps-deployment.md#update-an-existing-deployment).

The library exposes `config::{ClientSettings, ServerSettings}` with async
`load`, `save`, and `load_or_create` methods returning `io::Result`, plus
`validate` methods that report all validation problems together. Configured
server credential providers reject unknown identities, excessive clock skew, and
non-increasing authentication timestamps using MariaDB transactions. Replay
history survives server restarts. `ServerSettings::credentials().await` connects
to the database and applies embedded migrations; reuse the returned provider
across connections. The codec's nonce replay guard is shared by all connections
accepted by the listener. See [database setup and replay semantics](database.md).
