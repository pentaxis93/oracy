# Oracy Backend Deployment

This directory contains the Oracy-owned deployment substrate for the Rust
backend. The `v0.1.2` recipe targets Fedora CoreOS with rootless Podman
Quadlet generators: the backend runs behind an Oracy-scoped Caddy reverse
proxy on a private Oracy ingress network.

The repository owns the image build, Quadlet service shape, Caddy ingress
fabric, and persistence templates. Operators own concrete host bindings such
as paths, image tags, credentials, public hostname, and the host policy that
allows rootless Caddy to bind public HTTPS ports.

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

The shipped Quadlet templates privately relabel the mounted backend config,
backend state, and Caddyfile for confined Podman containers on
SELinux-enforcing hosts. Use host paths dedicated to this Oracy service; do
not share those paths with other containers.

## Configure The Backend

Use [`examples/oracy.toml`](examples/oracy.toml) as the starting point for the
container-mounted backend config. The default container-internal listeners are:

- `0.0.0.0:8080` for the public API, reachable only from the Oracy ingress
  network in the shipped deployment.
- `0.0.0.0:9090` for operator metrics, published to host loopback by the
  backend Quadlet template.

Put the OpenAI credential in an environment file readable by the service
account, for example:

```sh
install -m 0600 /dev/null /var/lib/oracy/oracy.env
printf 'OPENAI_API_KEY=%s\n' "$OPENAI_API_KEY" > /var/lib/oracy/oracy.env
```

Render [`examples/Caddyfile.in`](examples/Caddyfile.in) with the public Oracy
API hostname and place the result at the host path supplied through
`@ORACY_CADDYFILE_PATH@`.

## Install Quadlets

Render the templates from [`quadlet/`](quadlet/) and
[`examples/`](examples/) into the service account's persistent configuration,
replacing `@...@` placeholders with host values and removing the `.in` suffix
from rendered files.

Quadlet artifacts:

- `quadlet/oracy.container.in` renders to `oracy.container`.
- `quadlet/oracy-data.volume.in` renders to `oracy-data.volume`.
- `quadlet/oracy-ingress.network` copies as `oracy-ingress.network`.
- `quadlet/oracy-caddy.container.in` renders to `oracy-caddy.container`.
- `quadlet/oracy-caddy-data.volume` copies as `oracy-caddy-data.volume`.
- `quadlet/oracy-caddy-config.volume` copies as `oracy-caddy-config.volume`.

Caddy artifact:

- `examples/Caddyfile.in` renders to the host path supplied as
  `@ORACY_CADDYFILE_PATH@`.

The rendered Quadlet files normally live in
`~/.config/containers/systemd/` for the service account. The canonical
deployment shape is:

- Backend container: `oracy`
- Reverse proxy container: `oracy-caddy`
- Shared ingress network unit: `oracy-ingress.network`
- Podman network name: `oracy-ingress`
- Backend DNS alias on that network: `oracy`
- Caddy TLS state volume: `oracy-caddy-data.volume`
- Caddy config state volume: `oracy-caddy-config.volume`
- Public site block: `@ORACY_PUBLIC_HOSTNAME@`

The backend joins `oracy-ingress.network` and exposes the public API to Caddy
as `http://oracy:8080`. Caddy owns the public host bindings:

```ini
PublishPort=80:80
PublishPort=443:443
PublishPort=443:443/udp
```

Because this is a user-scope rootless deployment, the host must allow
unprivileged low-port binding before Caddy starts. Standard rootless Linux
rejects host ports below `1024`; provision
`net.ipv4.ip_unprivileged_port_start=80` or lower through the host's persistent
sysctl mechanism. That sysctl is host-wide: it allows all unprivileged
processes on the host, not only `oracy-caddy`, to bind ports at or above the
configured floor.

Caddy persists TLS and runtime state through named Podman volumes. `/data`
carries certificates and other TLS state; `/config` carries persistent Caddy
configuration state. A stateless proxy container loses certificates on restart
and can create avoidable ACME rate-limit pressure.

After rendering:

```sh
systemctl --user daemon-reload
systemctl --user start oracy-ingress-network.service
systemctl --user start oracy-data-volume.service
systemctl --user start oracy-caddy-data-volume.service
systemctl --user start oracy-caddy-config-volume.service
systemctl --user start oracy.service
systemctl --user start oracy-caddy.service
systemctl --user status oracy.service
systemctl --user status oracy-caddy.service
```

The generated container and network services include `[Install]` relationships
for the user default target. For boot persistence under user-scope systemd,
enable lingering for the service account through the host provisioning
mechanism.

The operator metrics publish line intentionally stays on host loopback:

```ini
PublishPort=127.0.0.1:@ORACY_OPERATOR_HOST_PORT@:9090
```

Keep that loopback binding unless another protected operator network surface
is intentionally provided.

## Alternate Scenario Audit

The retained deployment recipe is the Oracy-scoped shared container network
using Caddy. It is the primary path because it lets an independent Oracy
operator deploy backend, ingress network, reverse proxy, and TLS state as one
self-contained unit. Multi-app hosts keep modularity by running per-app
ingress fabrics instead of sharing an operator-wide reverse proxy substrate.

The prior host-system reverse-proxy scenario is not retained as a deployment
recipe. It fits operators who already manage a host-wide ingress layer, but
that layer is operator-owned infrastructure rather than Oracy-owned substrate;
keeping it as an equal recipe would preserve the framing this deployment
surface now rejects.

The prior isolated-container reverse-proxy scenario is not retained as a
deployment recipe. It fits hosts where a proxy cannot share Oracy's network,
but it requires a non-loopback backend publish plus host-gateway and firewall
policy. That is a custom operator topology, not the complete Oracy-scoped
fabric shipped here.

## Cut Over From An Existing Deployment

Existing deployments can differ in version, process manager, container runtime,
proxy placement, storage layout, and whether the reverse proxy was bundled with
the old stack. Treat the old system as the previous deployment, previous
ingress, and previous state. Do not assume its shape when planning cutover.
If either strategy decouples a reverse proxy that was bundled with the previous
deployment, use this Oracy-scoped Caddy ingress substrate as the construction
reference for the new public path.

Decide the previous state disposition before touching ingress:

| Disposition | Classification | Operator commitment |
|-------------|----------------|---------------------|
| `preserve` | Reversible | Keep the previous state readable by the previous deployment until the new public path has passed validation and the rollback window is closed. |
| `capture for separate migration` | Reversible until the captured copy becomes the only retained source | Stop old writers long enough to take a consistent copy or archive for later migration. The migration tooling is out of scope for `v0.1.x`; this capture only preserves material for separate work. |
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

When `stop-then-replace` must reclaim canonical paths or names from the start,
preserve-via-backup is one realization of rollback-window retention: move the
previous substrate to a backup location before creating the replacement at the
canonical location. For example, move previous `/var/lib/oracy` to
`/var/lib/oracy.rollback`, then create the replacement `/var/lib/oracy` for
the new stack. Keep that backup substrate, along with the previous
configuration, secrets, routes, volumes, and host bindings required for
rollback, until the rollback window is intentionally closed. Operators may
choose a different rollback substrate, such as an off-host backup, when it
preserves the same rollback capability.

Rollback model differs by strategy. For `parallel-then-swap`, roll back ingress
first: restore the previous DNS, proxy route, load-balancer target, or host
publish that pointed to the previous deployment. Restart or re-enable the
previous deployment against preserved previous state, then validate the old
public path before making more changes. For `stop-then-replace`, stop the
replacement stack first, restore the previous ingress setting, then restart or
re-enable the previous deployment against preserved previous state and
configuration. Under either strategy, any writes accepted by the new stack
after public traffic reaches it are not automatically present in the previous
state; preserve the new state for separate reconciliation rather than deleting
it during rollback.

## Operator-Owned Values

- `@ORACY_IMAGE@`: the locally built image tag, such as `localhost/oracy:0.1.0`.
- `@ORACY_ENV_FILE@`: host path to an environment file containing
  `OPENAI_API_KEY`.
- `@ORACY_CONFIG_PATH@`: host path to the backend TOML configuration.
- `@ORACY_STATE_DIR@`: host directory that persists SQLite and accepted audio.
- `@ORACY_OPERATOR_HOST_PORT@`: host loopback port for metrics, normally
  `9090`.
- `@ORACY_CADDYFILE_PATH@`: host path to the rendered Caddyfile, such as
  `/var/lib/oracy/Caddyfile`.
- `@ORACY_PUBLIC_HOSTNAME@`: public DNS hostname for the Oracy API. No sensible
  default exists; operators provide the real hostname, such as
  `api.oracy.example`.
