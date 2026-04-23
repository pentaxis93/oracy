import 'package:drift/drift.dart';

/// Opens a database connection for the current platform.
QueryExecutor openConnection() {
  throw UnsupportedError(
    'No suitable database implementation was found on this platform.',
  );
}
