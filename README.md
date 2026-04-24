Oracy is the voice transcription product: `client/` contains the Flutter
application, `spec/` defines the public API contract, and `backend/` contains
the Rust backend service.

## Quality Gates

CI runs backend and frontend gates independently for changes under `backend/`
and `client/`.

Backend:

```sh
cd backend
cargo fmt --check
cargo clippy -- -D warnings
cargo test
```

Frontend:

```sh
cd client
flutter analyze
flutter test
```
