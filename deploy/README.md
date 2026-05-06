# Oracy Backend Deployment

This directory contains generic deployment artifacts for the Rust backend.
The repository owns the image build, service shape, and persistence template;
operators own concrete host bindings such as paths, ports, permissions, image
tags, and secrets.

## Build The Image

From the repository root:

```sh
podman build -t localhost/oracy:0.1.0 .
```

The image runs `oracy-backend` as PID 1 and includes `ffmpeg` and `ffprobe`.
Runtime state and secrets are not baked into the image.

## Prepare Host State

Choose a persistent host directory for Oracy state, then create the accepted
audio directory inside it:

```sh
mkdir -p /var/lib/oracy/accepted-audio
```

The backend process inside the container must be able to create and update
`/var/lib/oracy/oracy.sqlite` and files below `/var/lib/oracy/accepted-audio`.
On rootless Podman deployments, verify the host ownership and UID/GID mapping
used by the service account before starting the unit.

The shipped Quadlet template privately relabels the mounted config file and
state volume for confined Podman containers on SELinux-enforcing hosts. Use
host paths dedicated to this Oracy service; do not share those paths with other
containers.

## Configure The Backend

Use [`examples/oracy.toml`](examples/oracy.toml) as the starting point for the
container-mounted config. The default container-internal listeners are:

- `0.0.0.0:8080` for the public API.
- `0.0.0.0:9090` for operator metrics, published to host loopback by the
  Quadlet template.

Put the OpenAI credential in an environment file readable by the service
account, for example:

```sh
install -m 0600 /dev/null /var/lib/oracy/oracy.env
printf 'OPENAI_API_KEY=%s\n' "$OPENAI_API_KEY" > /var/lib/oracy/oracy.env
```

## Install Quadlets

Render the templates from [`quadlet/`](quadlet/) into
`~/.config/containers/systemd/` for the service account, replacing the
`@...@` placeholders with host values and removing the `.in` suffix:

- `quadlet/oracy.container.in` renders to `oracy.container`.
- `quadlet/oracy-data.volume.in` renders to `oracy-data.volume`.

Choose the public API network surface for the reverse proxy that will receive
client traffic:

- Host-system reverse proxy: keep Oracy published only to host loopback, for
  example `@ORACY_PUBLIC_PUBLISH@=127.0.0.1:8080:8080`, and proxy to
  `http://127.0.0.1:8080`.
- Shared container network: if the reverse proxy can join the same Docker or
  Podman network as Oracy, do not publish the public API on the host. When
  rendering `oracy.container`, leave the public `PublishPort=` directive out
  and add a shared network, for example:

  ```ini
  Network=oracy-proxy.network
  NetworkAlias=oracy
  ```

  For Podman Quadlets, provide the matching operator-owned
  `oracy-proxy.network` unit or replace the example with your existing network.
  Put the proxy on the same network and proxy to `http://oracy:8080`. Docker
  Compose services in the same project use shared service DNS by default; use
  the Compose service name or network alias `oracy` for the backend service.
- Isolated container reverse proxy: if the proxy cannot share Oracy's
  container network, publish Oracy on a host address reachable from that proxy,
  for example `@ORACY_PUBLIC_PUBLISH@=0.0.0.0:8080:8080`, and proxy through the
  runtime's host gateway name such as `host.containers.internal` for Podman or
  `host.docker.internal` for Docker.

  For Docker on Linux, add the host gateway name to the isolated proxy
  container with `--add-host=host.docker.internal:host-gateway` or, in Compose:

  ```yaml
  extra_hosts:
    - "host.docker.internal:host-gateway"
  ```

  Podman provides `host.containers.internal` automatically and does not need
  this Docker-specific mapping.

  A non-loopback publish such as `0.0.0.0:8080:8080` is reachability, not protection.
  Use it only with operator-managed firewall rules or equivalent host network
  policy, and verify that the port is blocked from untrusted networks before
  treating the binding as protected. Prefer binding to a specific private host
  interface over `0.0.0.0` when the deployment has one.

A proxy running in an isolated container cannot reach an Oracy port published
only on host loopback unless the proxy shares the host network namespace. In
that case, use the host-system pattern intentionally.

The operator metrics publish line intentionally defaults to host loopback:

```ini
PublishPort=127.0.0.1:@ORACY_OPERATOR_HOST_PORT@:9090
```

Keep that loopback binding unless another protected operator network surface
is intentionally provided.

After templating:

```sh
systemctl --user daemon-reload
systemctl --user start oracy.service
systemctl --user status oracy.service
```

The Quadlet template's `[Install]` relationship makes the generated service
part of the user default target at `daemon-reload` time. For boot persistence
under user-scope systemd, enable lingering for the service account through the
host provisioning mechanism.

## Operator-Owned Values

- `@ORACY_IMAGE@`: the locally built image tag, such as `localhost/oracy:0.1.0`.
- `@ORACY_ENV_FILE@`: host path to an environment file containing
  `OPENAI_API_KEY`.
- `@ORACY_CONFIG_PATH@`: host path to the backend TOML configuration.
- `@ORACY_STATE_DIR@`: host directory that persists SQLite and accepted audio.
- `@ORACY_PUBLIC_PUBLISH@`: public API publish rule, such as
  `127.0.0.1:8080:8080` or `0.0.0.0:8080:8080`. Omit the rendered public
  publish line for shared container-network proxy deployments.
- `@ORACY_OPERATOR_HOST_PORT@`: host loopback port for metrics, normally
  `9090`.
