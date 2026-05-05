Oracy catches voice and keeps it. The user records audio; Oracy produces a
**voice note** — a durable text artifact derived from that audio, kept on
the server so it can be searched, edited, organized with tags and sessions,
and returned to.

`v0.1.0` delivers the voice-note substrate: chunked-audio submission,
OpenAI-backed transcription, durable storage, edit history, timing
segments, server-side embeddings, free-text tags, optional sessions, and
unified history/search. The backend is Linux-only (POSIX); the client
ships on Android and web.

The contract of record is [`spec/api-contract.md`](spec/api-contract.md);
backend obligations are in
[`spec/backend-requirements.md`](spec/backend-requirements.md). When other
documentation drifts, the spec wins.

## Repository Layout

- [`backend/`](backend/) — Rust service.
- [`client/`](client/) — Flutter application.
- [`deploy/`](deploy/) — backend container and Quadlet deployment templates.
- [`spec/`](spec/) — API contract and backend requirements.

## Quality Gates

CI runs backend and frontend gates independently for changes under `backend/`
and `client/`.

Backend:

```sh
./scripts/backend-ci
```

Frontend:

```sh
cd client
flutter analyze
flutter test
```
