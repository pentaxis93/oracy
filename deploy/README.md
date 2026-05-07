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
  Network=ingress.network
  NetworkAlias=oracy
  ```

  For Podman Quadlets, provide the matching operator-owned `ingress.network`
  unit or replace the example with your existing network.
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

## Provision A Fresh Reverse Proxy Substrate

If the host does not already have a reverse proxy, provision the ingress fabric
as operator-owned infrastructure before joining Oracy to it. The ingress
network and proxy are not Oracy artifacts; Oracy only needs a private route to
the proxy.

For Podman Quadlet deployments, create an `ingress.network` Quadlet in the same
user-scope Quadlet directory as the persistent services:

```ini
[Unit]
Description=Operator ingress network

[Network]
NetworkName=ingress

[Install]
WantedBy=default.target
```

`NetworkName=ingress` gives the Podman network an operator-owned name instead
of an app-specific name. Render `oracy.container` with the shared-network
shape from the install section:

```ini
Network=ingress.network
NetworkAlias=oracy
```

Leave Oracy's public API `PublishPort=` directive out in this topology. The
proxy reaches the backend through `http://oracy:8080` on the ingress network,
and the public internet reaches only the proxy.

Create the Caddy state volumes in the same Quadlet directory:

```ini
[Unit]
Description=Operator reverse proxy TLS state

[Volume]
VolumeName=caddy-data
```

```ini
[Unit]
Description=Operator reverse proxy config state

[Volume]
VolumeName=caddy-config
```

The concrete proxy is operator scope. A Caddy Quadlet is one illustrative
containerized proxy shape. Because this user-scope rootless example publishes
host ports 80 and 443, the host must allow unprivileged low-port binding before
the proxy starts. Standard rootless Linux rejects host ports below 1024; for
this example, provision `net.ipv4.ip_unprivileged_port_start=80` or lower
through the host's persistent sysctl mechanism. That sysctl is host-wide: it
allows all unprivileged processes on the host, not only this Caddy container,
to bind ports at or above the configured floor.

```ini
[Unit]
Description=Operator reverse proxy

[Container]
Image=docker.io/library/caddy:2
ContainerName=ingress-proxy
Network=ingress.network
PublishPort=80:80
PublishPort=443:443
PublishPort=443:443/udp
Volume=/var/lib/caddy/Caddyfile:/etc/caddy/Caddyfile:ro,Z
Volume=caddy-data.volume:/data:rw,Z
Volume=caddy-config.volume:/config:rw,Z

[Service]
Restart=on-failure
RestartSec=5

[Install]
WantedBy=default.target
```

The Caddyfile for that proxy would route the public site to Oracy by its
network alias:

```caddyfile
oracy.example.com {
	reverse_proxy http://oracy:8080
}
```

Persist the proxy's TLS state. For Caddy, `/data` carries certificates and
other state, and `/config` carries persistent configuration state. The
`caddy-data.volume` and `caddy-config.volume` units above back those container
paths with named Podman volumes; a stateless proxy container loses
certificates on restart and can create avoidable ACME rate-limit pressure.

## Cut Over From An Existing Deployment

Existing deployments can differ in version, process manager, container runtime,
proxy placement, storage layout, and whether the reverse proxy was bundled with
the old stack. Treat the old system as the previous deployment, previous
ingress, and previous state. Do not assume its shape when planning cutover.
If either strategy decouples a reverse proxy that was bundled with the previous
deployment, use `Provision A Fresh Reverse Proxy Substrate` as the construction
reference for the new operator-owned ingress fabric.

Decide the previous state disposition before touching ingress:

| Disposition | Classification | Operator commitment |
|-------------|----------------|---------------------|
| `preserve` | Reversible | Keep the previous state readable by the previous deployment until the new public path has passed validation and the rollback window is closed. |
| `capture for separate migration` | Reversible until the captured copy becomes the only retained source | Stop old writers long enough to take a consistent copy or archive for later migration. The migration tooling is out of scope for `v0.1.0`; this capture only preserves material for separate work. |
| `discard` | `irreversible-state` | Remove previous state only after explicit operator acceptance that the old data and rollback surface are no longer needed. |

Choose the cutover strategy that matches the operator constraint:

| Strategy | Choose when | Tradeoff |
|----------|-------------|----------|
| `parallel-then-swap` | Public downtime avoidance matters most, and the operator can tolerate temporary parallel topology. | Requires private alternate ports, candidate ingress, and coexistence naming until the swap completes. |
| `stop-then-replace` | Brief downtime is acceptable, and canonical names, ports, routes, and topology should be true from the start. | Production traffic stops during replacement, and validation happens only after the new stack owns the public path. |

Classify every `parallel-then-swap` operation before running it:

| Phase | Classification | Required outcome |
|-------|----------------|------------------|
| pre-cutover validation | Reversible | The previous public path is healthy, the previous state disposition is chosen, rollback ownership is assigned, and the operator knows which previous ingress setting can be restored. |
| new-stack standup | Reversible | The new Oracy backend starts beside the previous deployment on private ports, networks, and state paths that do not steal production traffic. |
| ingress candidate wiring | Reversible | The new backend is reachable through a candidate route while the previous ingress still serves production. |
| candidate validation | Reversible | Health, authentication, transcription submission, history/search reads, and metrics are validated against the candidate route without depending on production DNS or the old public path being disabled. |
| swap | `irreversible-state` boundary | Public DNS, proxy routing, load-balancer target, or equivalent ingress ownership moves from the previous deployment to the new stack. New client writes may now land in the new state store. |
| post-cutover validation | `irreversible-state` boundary | The public Oracy URL, operator metrics path, retained-audio path, and SQLite persistence path validate on the new stack after real ingress has moved. |
| decommission | `irreversible-state` once state or rollback surfaces are removed | Disable the previous deployment only after post-cutover validation passes. Remove previous state, secrets, routes, volumes, or host bindings only after the rollback window is intentionally closed. |

Classify every `stop-then-replace` operation before running it:

| Phase | Classification | Required outcome |
|-------|----------------|------------------|
| pre-cutover validation | Reversible | The previous public path is healthy, the previous state disposition is chosen, rollback ownership is assigned, and the operator knows which previous deployment command and ingress setting can be restored. |
| stop previous deployment | Reversible while previous state, configuration, and ingress settings are retained | The previous deployment is intentionally stopped, old writers are quiesced, and public Oracy downtime is accepted for the replacement window. |
| new-stack standup | Reversible until public writes are accepted | The new Oracy backend, state path, and ingress are built with canonical names, ports, routes, and network aliases instead of temporary coexistence values. |
| production validation | `irreversible-state` boundary once public writes are accepted | The public Oracy URL, authentication, transcription submission, history/search reads, operator metrics path, retained-audio path, and SQLite persistence path validate on the replacement stack. Stop-then-replace has no separate candidate validation phase because validation happens through the production path. |
| rollback-window retention | Reversible | The previous state, configuration, secrets, routes, volumes, and host bindings remain available until the operator accepts the replacement and closes the rollback window. |
| decommission | `irreversible-state` once state or rollback surfaces are removed | Remove previous state, secrets, routes, volumes, or host bindings only after production validation passes and the rollback window is intentionally closed. |

When `stop-then-replace` must reclaim canonical paths or names from the
start, preserve-via-backup is one realization of rollback-window retention:
move the previous substrate to a backup location before creating the
replacement at the canonical location. For example, move previous
`/var/lib/oracy` to `/var/lib/oracy.rollback`, then create the replacement
`/var/lib/oracy` for the new stack. Keep that backup substrate, along with the
previous configuration, secrets, routes, volumes, and host bindings required
for rollback, until the rollback window is intentionally closed. Operators may
choose a different rollback substrate, such as an off-host backup, when it
preserves the same rollback capability.

Rollback model differs by strategy. For `parallel-then-swap`, roll back ingress
first: restore the previous DNS, proxy route, load-balancer target, or host
publish that pointed to the previous deployment. Restart or re-enable the
previous deployment against preserved previous state, then validate the old
public path before making more changes. For `stop-then-replace`, stop the
replacement stack first, restore the previous ingress setting, then restart or
re-enable the previous deployment against preserved previous state and
configuration. Under either strategy, any writes accepted by the new stack after
public traffic reaches it are not automatically present in the previous state;
preserve the new state for separate reconciliation rather than deleting it
during rollback.

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
