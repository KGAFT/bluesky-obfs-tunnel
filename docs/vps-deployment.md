# Precompile the server and deploy to a VPS

Build the server on your computer and upload its Docker image. The VPS needs
Docker Engine and the Compose plugin, with no Rust compiler or source tree.
MariaDB stores client credentials and replay timestamps in a persistent volume.

## 1. Build the deployment bundle locally

Check the VPS architecture, replacing the SSH login below:

```sh
ssh your-user@your-vps 'uname -m'
```

Use `linux/amd64` for `x86_64`, or `linux/arm64` for `aarch64`. Your Docker
Buildx builder must support that platform, through a matching CPU or emulation.
From the repository root on your computer:

```sh
bash scripts/build_vps.sh linux/amd64
```

This builds only the server, with Diesel, embedded migrations, and runtime
libraries. Docker builds use the platform's baseline CPU; workstation Cargo
flags do not enter the image. The script writes:

- `dist/vps/server-image.tar`: the image to load on the VPS.
- `dist/vps/compose.yaml`: image-only services; it cannot compile on the VPS.
- `dist/vps/config/`: example settings, both patterns, and `patterns.sha256`.
- `dist/bluesky-server-vps.tar.gz`: the complete initial deployment bundle.

The bundled examples and root pattern pair use Chrome 149 with
`https://www.microsoft.com/`. Both peers must use the same cover settings and
pattern pair. To package patterns captured by an existing deployment, set
`PATTERN_DIR=/path/to/pattern-directory` when invoking the script and keep that
deployment's matching cover/browser settings. Never mix files from two captures.
The bundle includes examples only; it does not include runtime JSON or `.env`.

## 2. Upload and configure a new deployment

On your computer:

```sh
scp dist/bluesky-server-vps.tar.gz your-user@your-vps:~/
ssh your-user@your-vps
```

On the VPS, with Docker and Compose installed and accessible to your SSH user:

```sh
mkdir -p ~/bluesky-server
chmod 700 ~/bluesky-server
cd ~/bluesky-server
tar -xzf ~/bluesky-server-vps.tar.gz
docker image load --input server-image.tar
cp .env.example .env
cp config/server.example.json config/server.json
chmod 600 .env config/server.json
id -u
id -g
openssl rand -hex 32
(cd config && sha256sum -c patterns.sha256)
```

Edit `.env` before continuing:

- Set `MARIADB_PASSWORD` to the generated hex value.
- For a non-root deployment login, set `TUNNEL_UID` and `TUNNEL_GID` to the
  printed IDs. For a root login, keep the default `1000:1000` container user
  and assign the config files to it with the ownership step below.
- Keep `SERVER_CONFIG=./config/server.json`; this file must already exist.
- Set `TUNNEL_PORT` to the public listening port, normally 443.

Edit `config/server.json`: choose your own `obfuscation.public_password` and
`obfuscation.server_id`, then match them in the client. Keep multiplexed
forwarding and `bind_address` at `0.0.0.0:443`; Compose maps `TUNNEL_PORT` to
container port 443. For an internet-only proxy, set
`allow_local_proxy_connections` to `false` and list the VPS's public addresses
in `public_ips`. Add intended private targets to `local_proxy_allowlist` when
needed. Container loopback refers to the server container.

Compose mounts the JSON read-only at `/app/server.json` and `config/` writable
at `/app/config`, the working directory. Keep the default relative pattern
paths. The configured UID/GID must be able to read the JSON and write the
pattern directory and files. A missing JSON now fails as a missing bind source.
A missing or invalid pattern pair triggers capture and saving in `config/`;
existing valid patterns are reused even after changing cover settings.

Before starting the server or creating clients, align ownership with the
configured container user. From the deployment directory, run the following as
root (or use `sudo` for `chown` and `chmod`). Python 3 reads only the rendered
container user from Compose, without printing database credentials:

```sh
tunnel_owner="$(docker compose config --format json | python3 -c 'import json, sys; print(json.load(sys.stdin)["services"]["server"]["user"])')"
chown -R "$tunnel_owner" config
chmod 700 config
chmod 600 config/server.json
```

Repeat the ownership step after replacing files as root or extracting a new
archive. Archive ownership may come from the build computer; a root-created
`server.json` with mode `0600` is unreadable by container UID 1000 until its
ownership is corrected. Keep `.env` private to the deployment login.

## 3. Start MariaDB and create clients

```sh
docker compose config --quiet
docker compose up -d --wait db
```

Create a client named `alice` using this Bash password prompt. Run it on the
VPS from the deployment directory; a fresh SSH login starts in your home
directory, where Compose cannot find this project's `compose.yaml`:

```bash
cd ~/bluesky-server
read -r -s -p 'Client password: ' CLIENT_PASSWORD
echo
export CLIENT_PASSWORD
docker compose run --rm -e CLIENT_PASSWORD server --create-client alice
unset CLIENT_PASSWORD
```

This applies migrations, saves the client, and exits without binding the tunnel
port or contacting the cover site. Repeat with a different identity for each
client. Duplicate identities are rejected without resetting passwords or replay
history. The server image has `pull_policy: never`; load it before this step.
The Compose entrypoint selects `/app/server.json` explicitly, including when
`--create-client` replaces the normal command arguments.

## 4. Start and verify the tunnel

```sh
docker compose up -d --no-build server
docker compose ps
docker compose logs --tail 50 server
```

Wait for `Tunnel server listening on 0.0.0.0:443` in the logs. A running container
alone does not establish that pattern capture or authentication works. Allow
inbound TCP on `TUNNEL_PORT` in the provider and host firewalls. Allow outbound
DNS and HTTPS to the configured cover site (`www.microsoft.com:443` by default)
and the intended forwarding destinations. MariaDB has no public port. Keep
both peers' clocks synchronized for the 60-second replay window.

On your computer, copy the client example to a private `client.json`. Set
`server_address` to `VPS_ADDRESS:TUNNEL_PORT`, `client_identity` to `alice`,
and `client_password` to the value entered above. Match the server's public
password, server ID, cover URL/destination, and browser profile. Retrieve the
actual server pair, including after any automatic capture:

```sh
scp your-user@your-vps:~/bluesky-server/config/cli_pattern.bin ./cli_pattern.bin
scp your-user@your-vps:~/bluesky-server/config/serv_pattern.bin ./serv_pattern.bin
sha256sum cli_pattern.bin serv_pattern.bin
./client --config client.json --http-proxy 127.0.0.1:8080
```

Run from the directory containing those patterns, or set absolute pattern paths
in the JSON. In another terminal, verify HTTP and HTTPS through the proxy:

```sh
curl --noproxy '' --proxy http://127.0.0.1:8080 http://example.com/
curl --noproxy '' --proxy http://127.0.0.1:8080 https://example.com/
```

Both services restart after a VPS reboot if Docker is enabled at boot.
Credentials and replay timestamps remain in `mariadb_data`. Keep the same
Compose project/directory across updates; do not delete its volume.

## Fix config permission errors

For `cannot read server.json: Permission denied` (or the same error for
`/app/server.json`), compare the configured container user with the file owner:

```sh
cd ~/bluesky-server
stat -c '%u:%g %a %n' config config/server.json
docker compose run --rm --no-deps --workdir /app --entrypoint id server
```

On the verified VPS deployment, the file was `0:0` with mode `600`, while the
container used `1000:1000`. The ownership step in section 2 corrected access
and kept the config private. After applying it, verify read/write access from
inside the actual container user and recreate the server:

```sh
docker compose run --rm --no-deps --entrypoint /bin/sh server -ec \
  'test -r /app/server.json; test -w /app/config; echo config-permissions-ok'
docker compose up -d --no-build --force-recreate server
docker compose logs --tail 30 server
```

Successful startup logs `Loaded config from /app/server.json` and
`Tunnel server listening on 0.0.0.0:443`. If an older image or Compose file
still loads relative `server.json` during client creation, use
`server --config /app/server.json --create-client alice`, or update Compose to
the current entrypoint. The container must be able to read the mounted JSON in
both cases. Client creation requires `CLIENT_PASSWORD` as shown above.

## Update an existing deployment

Build a fresh bundle locally with `scripts/build_vps.sh`, then upload the image
and current Compose file:

```sh
scp dist/vps/server-image.tar dist/vps/compose.yaml your-user@your-vps:~/bluesky-server/
```

Before updating, back up the existing `.env`, server JSON, pattern pair, and
MariaDB. For a consistent database dump on the VPS:

```sh
cd ~/bluesky-server
umask 077
docker compose exec -T db sh -c \
  'exec mariadb-dump --user="$MARIADB_USER" --password="$MARIADB_PASSWORD" --single-transaction "$MARIADB_DATABASE"' \
  > "database-backup-$(date -u +%Y%m%dT%H%M%SZ).sql"
docker image tag bluesky-obfs-tunnel-server:local bluesky-obfs-tunnel-server:previous
docker image load --input server-image.tar
docker compose config --quiet
docker compose up -d --no-build --force-recreate server
docker compose logs --tail 50 server
```

Keep your existing `.env`, runtime JSON, and patterns during a binary-only
update. Migrations run at startup. Changing `MARIADB_PASSWORD` in `.env` does
not rotate an existing database user's password. Existing tunnels disconnect
on recreation. Retest both curl requests after the update. If the new binary
fails, restore the previous image tag and recreate the server, provided the
schema remains compatible:

```sh
docker image tag bluesky-obfs-tunnel-server:previous bluesky-obfs-tunnel-server:local
docker compose up -d --no-build --force-recreate server
```

For deployments from before these fixes:

- Remove the old server JSON `clients` array, add `database` from the current
  example, and create those identities once with `--create-client`. Existing
  database identities need no recreation.
- Select `{"mode":"multiplexed"}` for the HTTP proxy and continuous TCP/UDP client forwarding.
- Check the actual file selected by `SERVER_CONFIG` or `--config`; existing JSON
  retains old settings, including copies under `target/release/`.
- Set the host UID/GID for writable `config/`, and keep MariaDB private. The
  base Compose file is now image-only; local source builds use
  `docker compose -f compose.yaml -f compose.build.yaml build server`.
- To adopt Chrome 149/Microsoft, update `obfuscation.target_sni`,
  `target_sni_connection_dest`, `browser_emulation`, `patterns.capture_dest`, and
  `capture_url` on both peers. Stop the server, back up both existing patterns,
  and replace them together with the matching new pair (or move both aside to
  trigger capture). Recreate the server and distribute its final pair to all
  clients, including Termux packages. Changing JSON alone never refreshes valid
  patterns. Do not overwrite an established deployment's pair on every build.

Native builds retain the optimization flags in `.cargo/config.toml`, including
AVX/AVX2 and `opt-level=3`; they require a CPU supporting the enabled features.
Docker builds omit that file, and Termux builds override its flags for ARM64.
Existing binaries and previously built deployment archives are not updated by
changing source files.
