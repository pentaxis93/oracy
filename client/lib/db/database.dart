import 'package:drift/drift.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';
import 'package:uuid/uuid.dart';

// Platform-specific database connection
import 'connection/connection.dart'
    if (dart.library.html) 'connection/web.dart'
    if (dart.library.io) 'connection/native.dart';

part 'database.g.dart';

final _uuid = const Uuid();

/// Status of a pending upload.
enum UploadStatus {
  /// Waiting to be uploaded.
  pending,

  /// Currently uploading.
  uploading,

  /// Upload failed, can be retried.
  failed,

  /// Upload failed permanently and should not be retried automatically.
  terminalFailure,

  /// Upload succeeded.
  completed,

  /// Upload succeeded, but local file cleanup must be retried.
  cleanupPending,
}

/// Table for storing pending audio uploads for offline sync.
class PendingUploads extends Table {
  /// Auto-incrementing primary key.
  IntColumn get id => integer().autoIncrement()();

  /// Path to the audio file on disk.
  TextColumn get audioPath => text()();

  /// When this upload was queued.
  DateTimeColumn get createdAt => dateTime().withDefault(currentDateAndTime)();

  /// Number of retry attempts.
  IntColumn get retryCount => integer().withDefault(const Constant(0))();

  /// Current status.
  IntColumn get status =>
      integer().withDefault(Constant(UploadStatus.pending.index))();

  /// Optional error message from last failed attempt.
  TextColumn get errorMessage => text().nullable()();

  /// When this record was last updated.
  DateTimeColumn get updatedAt => dateTime().nullable()();

  /// Optional language hint for transcription.
  TextColumn get language => text().nullable()();

  /// Stable idempotency key reused across retries for the same recording.
  TextColumn get idempotencyKey => text().nullable()();

  @override
  List<Set<Column>> get uniqueKeys => [
    {audioPath},
  ];
}

@DriftDatabase(tables: [PendingUploads])
class AppDatabase extends _$AppDatabase {
  AppDatabase([QueryExecutor? executor]) : super(executor ?? _openConnection());

  @override
  int get schemaVersion => 3;

  @override
  MigrationStrategy get migration => MigrationStrategy(
    onUpgrade: (Migrator m, int from, int to) async {
      if (from < 2) {
        await m.addColumn(pendingUploads, pendingUploads.idempotencyKey);
      }
      if (from < 3) {
        await _dedupePendingUploadsByAudioPath();
        await customStatement(
          'CREATE UNIQUE INDEX IF NOT EXISTS pending_uploads_audio_path_unique '
          'ON pending_uploads (audio_path)',
        );
        await _backfillMissingIdempotencyKeys();
      }
    },
  );

  /// Get all pending uploads (status = pending or failed with retryCount < max).
  Future<List<PendingUpload>> getPendingUploads({int maxRetries = 3}) {
    return (select(pendingUploads)
          ..where(
            (t) =>
                t.status.equals(UploadStatus.pending.index) |
                (t.status.equals(UploadStatus.failed.index) &
                    t.retryCount.isSmallerThanValue(maxRetries)),
          )
          ..orderBy([(t) => OrderingTerm.asc(t.createdAt)]))
        .get();
  }

  /// Add a new pending upload.
  Future<int> addPendingUpload({
    required String audioPath,
    String? language,
    String? idempotencyKey,
  }) {
    return into(pendingUploads).insert(
      PendingUploadsCompanion.insert(
        audioPath: audioPath,
        language: Value(language),
        idempotencyKey: Value(idempotencyKey ?? _uuid.v4()),
      ),
    );
  }

  /// Find an upload entry by audio path.
  Future<PendingUpload?> getUploadByAudioPath(String audioPath) {
    return (select(
      pendingUploads,
    )..where((t) => t.audioPath.equals(audioPath))).getSingleOrNull();
  }

  /// Find an upload entry by ID.
  Future<PendingUpload?> getUploadById(int id) {
    return (select(
      pendingUploads,
    )..where((t) => t.id.equals(id))).getSingleOrNull();
  }

  /// Ensure an upload is queued exactly once for this audio path.
  Future<int> ensurePendingUpload({
    required String audioPath,
    String? language,
  }) async {
    return transaction(() async {
      await into(pendingUploads).insert(
        PendingUploadsCompanion.insert(
          audioPath: audioPath,
          language: Value(language),
          idempotencyKey: Value(_uuid.v4()),
        ),
        mode: InsertMode.insertOrIgnore,
      );

      final existing = await getUploadByAudioPath(audioPath);
      if (existing == null) {
        throw StateError('Pending upload disappeared for $audioPath');
      }

      if (existing.idempotencyKey == null || existing.idempotencyKey!.isEmpty) {
        throw StateError(
          'Pending upload $audioPath is missing its idempotency key.',
        );
      }

      final languageChanged = language != null && existing.language != language;

      if (languageChanged) {
        await (update(
          pendingUploads,
        )..where((t) => t.id.equals(existing.id))).write(
          PendingUploadsCompanion(
            language: languageChanged ? Value(language) : const Value.absent(),
            updatedAt: Value(DateTime.now()),
          ),
        );
      }

      return existing.id;
    });
  }

  /// Update upload status.
  Future<int> updateUploadStatus(
    int id,
    UploadStatus status, {
    String? errorMessage,
  }) {
    return (update(pendingUploads)..where((t) => t.id.equals(id))).write(
      PendingUploadsCompanion(
        status: Value(status.index),
        errorMessage: Value(errorMessage),
        updatedAt: Value(DateTime.now()),
        retryCount: status == UploadStatus.failed
            ? const Value.absent() // Will be incremented separately
            : const Value.absent(),
      ),
    );
  }

  /// Increment retry count for a failed upload.
  Future<int> incrementRetryCount(int id, {String? errorMessage}) async {
    final upload = await (select(
      pendingUploads,
    )..where((t) => t.id.equals(id))).getSingleOrNull();
    if (upload == null) return 0;

    return (update(pendingUploads)..where((t) => t.id.equals(id))).write(
      PendingUploadsCompanion(
        retryCount: Value(upload.retryCount + 1),
        status: Value(UploadStatus.failed.index),
        errorMessage: Value(errorMessage),
        updatedAt: Value(DateTime.now()),
      ),
    );
  }

  /// Mark upload as permanently failed and excluded from automatic retries.
  Future<int> markAsTerminalFailure(int id, {String? errorMessage}) {
    return (update(pendingUploads)..where((t) => t.id.equals(id))).write(
      PendingUploadsCompanion(
        status: Value(UploadStatus.terminalFailure.index),
        errorMessage: Value(errorMessage),
        updatedAt: Value(DateTime.now()),
      ),
    );
  }

  /// Mark upload as currently uploading.
  Future<int> markAsUploading(int id) {
    return updateUploadStatus(id, UploadStatus.uploading);
  }

  /// Restore an upload to the pending queue.
  Future<int> markAsPending(int id) {
    return updateUploadStatus(id, UploadStatus.pending);
  }

  /// Mark upload as completed and optionally delete the record.
  Future<int> markAsCompleted(int id, {bool deleteRecord = true}) async {
    if (deleteRecord) {
      return (delete(pendingUploads)..where((t) => t.id.equals(id))).go();
    }
    return updateUploadStatus(id, UploadStatus.completed);
  }

  /// Mark upload as needing local file cleanup before record removal.
  Future<int> markAsCleanupPending(int id, {String? errorMessage}) {
    return updateUploadStatus(
      id,
      UploadStatus.cleanupPending,
      errorMessage: errorMessage,
    );
  }

  /// Restore uploading rows that appear stranded based on their last update time.
  Future<int> restoreStaleUploadingUploads(Duration threshold) {
    final cutoff = DateTime.now().subtract(threshold);
    return (update(pendingUploads)..where(
          (t) =>
              t.status.equals(UploadStatus.uploading.index) &
              (t.updatedAt.isNull() | t.updatedAt.isSmallerThanValue(cutoff)),
        ))
        .write(
          PendingUploadsCompanion(
            status: Value(UploadStatus.failed.index),
            errorMessage: const Value('Upload interrupted before completion.'),
            updatedAt: Value(DateTime.now()),
          ),
        );
  }

  /// Delete a pending upload by ID.
  Future<int> deletePendingUpload(int id) {
    return (delete(pendingUploads)..where((t) => t.id.equals(id))).go();
  }

  /// Get uploads whose server work succeeded but local file cleanup still needs retrying.
  Future<List<PendingUpload>> getCleanupPendingUploads() {
    return (select(pendingUploads)
          ..where((t) => t.status.equals(UploadStatus.cleanupPending.index))
          ..orderBy([(t) => OrderingTerm.asc(t.createdAt)]))
        .get();
  }

  /// Watch terminal failures in creation order for manual recovery actions.
  Stream<List<PendingUpload>> watchTerminalFailureUploads() {
    return (select(pendingUploads)
          ..where((t) => t.status.equals(UploadStatus.terminalFailure.index))
          ..orderBy([(t) => OrderingTerm.asc(t.createdAt)]))
        .watch();
  }

  /// Get count of unsynced recordings, including terminal failures.
  Future<int> getPendingCount() async {
    final count = pendingUploads.id.count();
    final query = selectOnly(pendingUploads)
      ..where(
        pendingUploads.status.equals(UploadStatus.pending.index) |
            pendingUploads.status.equals(UploadStatus.failed.index) |
            pendingUploads.status.equals(UploadStatus.terminalFailure.index),
      )
      ..addColumns([count]);
    final result = await query.getSingle();
    return result.read(count) ?? 0;
  }

  /// Watch unsynced recording count for UI updates.
  Stream<int> watchPendingCount() {
    final count = pendingUploads.id.count();
    final query = selectOnly(pendingUploads)
      ..where(
        pendingUploads.status.equals(UploadStatus.pending.index) |
            pendingUploads.status.equals(UploadStatus.failed.index) |
            pendingUploads.status.equals(UploadStatus.terminalFailure.index),
      )
      ..addColumns([count]);
    return query.watchSingle().map((row) => row.read(count) ?? 0);
  }

  /// Clean up old completed/failed uploads older than specified duration.
  Future<int> cleanupOldUploads({Duration maxAge = const Duration(days: 7)}) {
    final cutoff = DateTime.now().subtract(maxAge);
    return (delete(pendingUploads)..where(
          (t) =>
              t.status.equals(UploadStatus.completed.index) &
              t.createdAt.isSmallerThanValue(cutoff),
        ))
        .go();
  }

  Future<void> _dedupePendingUploadsByAudioPath() async {
    final uploads = await select(pendingUploads).get();
    final uploadsByPath = <String, List<PendingUpload>>{};

    for (final upload in uploads) {
      uploadsByPath.putIfAbsent(upload.audioPath, () => []).add(upload);
    }

    for (final duplicates in uploadsByPath.values) {
      if (duplicates.length < 2) {
        continue;
      }

      duplicates.sort(_comparePendingUploadsForDeduplication);
      final survivor = duplicates.first;
      final duplicateIds = duplicates
          .skip(1)
          .map((upload) => upload.id)
          .toList();
      final mergedLanguage =
          survivor.language ??
          _firstNonEmptyString(duplicates.map((upload) => upload.language));
      final mergedIdempotencyKey =
          survivor.idempotencyKey ??
          _firstNonEmptyString(
            duplicates.map((upload) => upload.idempotencyKey),
          );
      final mergedErrorMessage =
          survivor.errorMessage ??
          _firstNonEmptyString(duplicates.map((upload) => upload.errorMessage));
      final mergedRetryCount = duplicates
          .map((upload) => upload.retryCount)
          .reduce((left, right) => left > right ? left : right);
      final mergedCreatedAt = duplicates
          .map((upload) => upload.createdAt)
          .reduce((left, right) => left.isBefore(right) ? left : right);
      final mergedUpdatedAt = duplicates
          .map((upload) => upload.updatedAt)
          .whereType<DateTime>()
          .fold<DateTime?>(null, (current, next) {
            if (current == null || next.isAfter(current)) {
              return next;
            }
            return current;
          });

      await (update(
        pendingUploads,
      )..where((t) => t.id.equals(survivor.id))).write(
        PendingUploadsCompanion(
          createdAt: Value(mergedCreatedAt),
          retryCount: Value(mergedRetryCount),
          errorMessage: Value(mergedErrorMessage),
          updatedAt: Value(mergedUpdatedAt),
          language: Value(mergedLanguage),
          idempotencyKey: Value(mergedIdempotencyKey),
        ),
      );

      await (delete(
        pendingUploads,
      )..where((t) => t.id.isIn(duplicateIds))).go();
    }
  }

  Future<void> _backfillMissingIdempotencyKeys() async {
    final uploadsMissingKeys = (await select(
      pendingUploads,
    ).get()).where((upload) {
      final key = upload.idempotencyKey;
      return key == null || key.isEmpty;
    });

    for (final upload in uploadsMissingKeys) {
      await (update(
        pendingUploads,
      )..where((t) => t.id.equals(upload.id))).write(
        PendingUploadsCompanion(
          idempotencyKey: Value(_uuid.v4()),
          updatedAt: Value(DateTime.now()),
        ),
      );
    }
  }

  static int _comparePendingUploadsForDeduplication(
    PendingUpload left,
    PendingUpload right,
  ) {
    final statusComparison = _dedupeStatusPriority(
      right.status,
    ).compareTo(_dedupeStatusPriority(left.status));
    if (statusComparison != 0) {
      return statusComparison;
    }

    final idempotencyComparison = _hasValue(
      right.idempotencyKey,
    ).compareTo(_hasValue(left.idempotencyKey));
    if (idempotencyComparison != 0) {
      return idempotencyComparison;
    }

    final leftTimestamp = left.updatedAt ?? left.createdAt;
    final rightTimestamp = right.updatedAt ?? right.createdAt;
    final recencyComparison = rightTimestamp.compareTo(leftTimestamp);
    if (recencyComparison != 0) {
      return recencyComparison;
    }

    return left.id.compareTo(right.id);
  }

  static int _dedupeStatusPriority(int status) => switch (status) {
    5 => 5,
    4 => 4,
    1 => 3,
    2 => 2,
    0 => 1,
    3 => 0,
    _ => -1,
  };

  static int _hasValue(String? value) => value == null || value.isEmpty ? 0 : 1;

  static String? _firstNonEmptyString(Iterable<String?> values) {
    for (final value in values) {
      if (value != null && value.isNotEmpty) {
        return value;
      }
    }
    return null;
  }
}

QueryExecutor _openConnection() {
  return openConnection();
}

/// Provider for the app database.
final appDatabaseProvider = Provider<AppDatabase>((ref) {
  final db = AppDatabase();
  ref.onDispose(() => db.close());
  return db;
});
