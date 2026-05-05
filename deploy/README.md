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
  `127.0.0.1:8080:8080` or `0.0.0.0:8080:8080`.
- `@ORACY_OPERATOR_HOST_PORT@`: host loopback port for metrics, normally
  `9090`.
