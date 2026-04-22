import 'dart:io';

import 'package:drift/native.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:oracy/db/database.dart';
import 'package:sqlite3/sqlite3.dart' as sqlite;

void main() {
  late Directory tempDir;
  late File dbFile;

  setUp(() async {
    tempDir = await Directory.systemTemp.createTemp(
      'oracy_database_migration_test_',
    );
    dbFile = File('${tempDir.path}/oracy.sqlite');
  });

  tearDown(() async {
    if (await tempDir.exists()) {
      await tempDir.delete(recursive: true);
    }
  });

  test(
    'Given a schema 2 database with null idempotency keys, When it is upgraded, Then every pending upload receives a UUID key',
    () async {
      final legacyDb = sqlite.sqlite3.open(dbFile.path);
      legacyDb.execute('''
        CREATE TABLE pending_uploads (
          id INTEGER PRIMARY KEY AUTOINCREMENT,
          audio_path TEXT NOT NULL,
          created_at INTEGER NOT NULL,
          retry_count INTEGER NOT NULL DEFAULT 0,
          status INTEGER NOT NULL DEFAULT 0,
          error_message TEXT,
          updated_at INTEGER,
          language TEXT,
          idempotency_key TEXT
        );
      ''');
      legacyDb.execute('''
        INSERT INTO pending_uploads (
          audio_path,
          created_at,
          retry_count,
          status,
          error_message,
          updated_at,
          language,
          idempotency_key
        ) VALUES
          ('/tmp/legacy_one.wav', 1713300000000, 0, 0, NULL, NULL, 'en', NULL),
          ('/tmp/legacy_two.wav', 1713300001000, 2, 2, 'timeout', 1713300002000, NULL, NULL);
      ''');
      legacyDb.execute('PRAGMA user_version = 2;');
      legacyDb.dispose();

      final db = AppDatabase(NativeDatabase(dbFile));
      addTearDown(db.close);

      final uploads = await db.getPendingUploads(maxRetries: 10);

      expect(uploads, hasLength(2));
      for (final upload in uploads) {
        expect(upload.idempotencyKey, isNotNull);
        expect(
          upload.idempotencyKey,
          matches(
            RegExp(
              r'^[0-9a-f]{8}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{12}$',
            ),
          ),
        );
      }
      expect(
        uploads.map((upload) => upload.idempotencyKey).toSet(),
        hasLength(2),
      );
    },
  );
}
