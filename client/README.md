`client/` contains Oracy's Flutter application. Build the app with `cd client && flutter pub get`. For v0.1.0, the client ships on Android and web only; Linux, Windows, macOS, and iOS are explicit non-scope targets. This code is imported as a snapshot from the Python-era repository and is being adapted to the contract in [`spec/api-contract.md`](../spec/api-contract.md): history/search reads use the v0.1.0 voice-note collection contract, and upload submission uses the v0.1.0 chunked transcription-job protocol. The checked-in web runtime assets such as `web/drift_worker.dart.js`, `web/drift_worker.dart.js.map`, and `web/sqlite3.wasm` are intentional generated or vendored snapshot artifacts and should not be hand-edited.

## API server URL

The shipped default API server is `https://api.oracy.app`. Operators distributing a client for a different backend can change the install's default at build time:

```sh
flutter build apk --dart-define=ORACY_API_BASE_URL=https://staging.oracy.app
```

Users and developers can also change the server URL at runtime from Settings. Runtime configuration wins over the build-time default until the user resets it. Changing the effective server URL clears the stored API key so credentials for one backend are not sent to another backend.
