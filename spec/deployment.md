# Oracy Deployment Contract

Target release: `v0.1.0`

## Purpose

This document defines the operator-provided resources required to run the
Oracy backend. It is the operator-side contract: what the deployment
environment must provide before the backend can start cleanly and preserve its
durability commitments under nominal load.

`backend/README.md` describes developer-facing configuration keys and startup
behavior. This document describes the infrastructure responsibilities behind
those settings.

## Audience

This document is for operators standing up Oracy on POSIX Linux
infrastructure. It assumes familiarity with filesystem permissions, persistent
volume management, environment-variable provisioning, and long-running backend
processes.

## Platform Scope

For `v0.1.0`, Oracy's backend assumes Linux with POSIX filesystem semantics.
The operator-provided storage described here must support atomic rename,
`fsync`, and standard POSIX permission bits. Network filesystems are
operator-risk: the backend assumes local-disk semantics for durability and
crash-safety.

## Accepted Audio Directory

The backend stores accepted audio chunks and composed audio in the configured
`accepted_audio_dir` while a transcription job is still active.

Operators must provide a directory with these properties:

- The path exists before backend startup.
- The path is on a POSIX filesystem.
- The directory is owned by the user the backend process runs as.
- That user has read, write, and execute permissions on the directory.
- Contents survive backend restarts.
- Contents are on a volume that persists across container or host reboots in
  the operator's deployment.

Capacity must cover active chunked-submission state. The directory holds chunks
while jobs are in `accepting_chunks`, plus composed audio while jobs are in
`queued`, `processing`, or `retry_waiting`. The backend releases retained audio
when the originating job reaches terminal `succeeded` or `failed`, so
steady-state size is bounded by concurrent in-flight submissions rather than
historical voice-note volume.

## SQLite Database File

The backend stores durable job, voice-note, settings, and metadata state in the
configured `database_path`.

Operators must provide a database path with these properties:

- The parent directory exists before backend startup.
- The parent directory is on a POSIX filesystem.
- The parent directory is owned by the user the backend process runs as.
- That user can create and write the database file and SQLite sidecar files.
- The database file and its WAL and SHM siblings survive backend restarts.
- The database file and its WAL and SHM siblings are on a volume that persists
  across container or host reboots in the operator's deployment.

SQLite relies on filesystem support for `fsync` and atomic rename for
crash-safety. Filesystems or mounts that weaken those semantics do not satisfy
Oracy's `v0.1.0` persistence assumptions.

## `OPENAI_API_KEY`

The backend uses `OPENAI_API_KEY` as the OpenAI transcription-engine
credential. The credential is read from the process environment at backend
startup.

Operators must provide the credential with these properties:

- The value is obtained from OpenAI and scoped to the backend's expected
  transcription usage.
- The value is present in the `OPENAI_API_KEY` environment variable whenever
  the backend process starts.
- The value is non-empty.
- The provisioning mechanism persists across backend restarts.

The mechanism is operator-owned. Examples include systemd unit environment
directives, container runtime env files, and secret-store-backed retrieval. The
contract is that the value reliably reaches the backend at every startup.

Credential rotation happens by updating the value that will be present in the
backend process environment and restarting the backend. The backend has no
in-process credential rotation mechanism. Operators implementing automated
rotation own the orchestration that updates the startup environment and
restarts the process.

Operators are responsible for protecting operator-controlled surfaces that
carry the credential. The value should not be exposed in version control,
shared logs, copied configuration, shell history, or diagnostic output produced
by provisioning and deployment tooling outside the backend.

## Media Tools

The backend uses FFmpeg tooling for audio duration probing and format-safe
splitting before OpenAI transcription requests. Operators must provide
`ffmpeg` and `ffprobe` on `PATH` for the backend process. Startup fails if
either tool is missing or cannot execute.

## Operator Metrics

The backend exposes Prometheus-compatible metrics from an operator listener
separate from the public API listener.

Operators must provision the operator listener with these properties:

- The listener is reachable by the Prometheus scraper.
- The listener is not exposed as an internet-facing public API surface.
- The default address is `127.0.0.1:9090`.
- `operator_listen_addr` may override the default when the scraper runs
  outside the backend host namespace.
- `operator_listen_addr` must not overlap the public `listen_addr` bind set.
- The scrape path is `GET /metrics`.
- The exposition format is Prometheus text format version `0.0.4`.
- The recommended scrape interval is `15s`.

Access control for v0.1.0 is network placement. The backend does not require
an application-level scrape token for `/metrics`; operators protect the surface
through loopback binding, firewall policy, reverse-proxy policy, or equivalent
deployment controls.

## Worked Example

`tesserine/ops` documents one concrete deployment pattern in
[`babbie-services.md`](https://github.com/tesserine/ops/blob/main/babbie-services.md).
That environment uses Podman Quadlets on Fedora CoreOS with user-scope systemd.

The babbie document is illustrative, not normative. Operators on different
infrastructure satisfy this contract through the storage, environment, and
process-management mechanisms appropriate to their own deployment.
