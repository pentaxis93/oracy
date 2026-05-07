// GENERATED CODE - DO NOT MODIFY BY HAND

part of 'database.dart';

// ignore_for_file: type=lint
class $PendingUploadsTable extends PendingUploads
    with TableInfo<$PendingUploadsTable, PendingUpload> {
  @override
  final GeneratedDatabase attachedDatabase;
  final String? _alias;
  $PendingUploadsTable(this.attachedDatabase, [this._alias]);
  static const VerificationMeta _idMeta = const VerificationMeta('id');
  @override
  late final GeneratedColumn<int> id = GeneratedColumn<int>(
    'id',
    aliasedName,
    false,
    hasAutoIncrement: true,
    type: DriftSqlType.int,
    requiredDuringInsert: false,
    defaultConstraints: GeneratedColumn.constraintIsAlways(
      'PRIMARY KEY AUTOINCREMENT',
    ),
  );
  static const VerificationMeta _audioPathMeta = const VerificationMeta(
    'audioPath',
  );
  @override
  late final GeneratedColumn<String> audioPath = GeneratedColumn<String>(
    'audio_path',
    aliasedName,
    false,
    type: DriftSqlType.string,
    requiredDuringInsert: true,
  );
  static const VerificationMeta _createdAtMeta = const VerificationMeta(
    'createdAt',
  );
  @override
  late final GeneratedColumn<DateTime> createdAt = GeneratedColumn<DateTime>(
    'created_at',
    aliasedName,
    false,
    type: DriftSqlType.dateTime,
    requiredDuringInsert: false,
    defaultValue: currentDateAndTime,
  );
  static const VerificationMeta _retryCountMeta = const VerificationMeta(
    'retryCount',
  );
  @override
  late final GeneratedColumn<int> retryCount = GeneratedColumn<int>(
    'retry_count',
    aliasedName,
    false,
    type: DriftSqlType.int,
    requiredDuringInsert: false,
    defaultValue: const Constant(0),
  );
  static const VerificationMeta _statusMeta = const VerificationMeta('status');
  @override
  late final GeneratedColumn<int> status = GeneratedColumn<int>(
    'status',
    aliasedName,
    false,
    type: DriftSqlType.int,
    requiredDuringInsert: false,
    defaultValue: Constant(UploadStatus.pending.index),
  );
  static const VerificationMeta _errorMessageMeta = const VerificationMeta(
    'errorMessage',
  );
  @override
  late final GeneratedColumn<String> errorMessage = GeneratedColumn<String>(
    'error_message',
    aliasedName,
    true,
    type: DriftSqlType.string,
    requiredDuringInsert: false,
  );
  static const VerificationMeta _updatedAtMeta = const VerificationMeta(
    'updatedAt',
  );
  @override
  late final GeneratedColumn<DateTime> updatedAt = GeneratedColumn<DateTime>(
    'updated_at',
    aliasedName,
    true,
    type: DriftSqlType.dateTime,
    requiredDuringInsert: false,
  );
  static const VerificationMeta _languageMeta = const VerificationMeta(
    'language',
  );
  @override
  late final GeneratedColumn<String> language = GeneratedColumn<String>(
    'language',
    aliasedName,
    true,
    type: DriftSqlType.string,
    requiredDuringInsert: false,
  );
  static const VerificationMeta _idempotencyKeyMeta = const VerificationMeta(
    'idempotencyKey',
  );
  @override
  late final GeneratedColumn<String> idempotencyKey = GeneratedColumn<String>(
    'idempotency_key',
    aliasedName,
    true,
    type: DriftSqlType.string,
    requiredDuringInsert: false,
  );
  @override
  List<GeneratedColumn> get $columns => [
    id,
    audioPath,
    createdAt,
    retryCount,
    status,
    errorMessage,
    updatedAt,
    language,
    idempotencyKey,
  ];
  @override
  String get aliasedName => _alias ?? actualTableName;
  @override
  String get actualTableName => $name;
  static const String $name = 'pending_uploads';
  @override
  VerificationContext validateIntegrity(
    Insertable<PendingUpload> instance, {
    bool isInserting = false,
  }) {
    final context = VerificationContext();
    final data = instance.toColumns(true);
    if (data.containsKey('id')) {
      context.handle(_idMeta, id.isAcceptableOrUnknown(data['id']!, _idMeta));
    }
    if (data.containsKey('audio_path')) {
      context.handle(
        _audioPathMeta,
        audioPath.isAcceptableOrUnknown(data['audio_path']!, _audioPathMeta),
      );
    } else if (isInserting) {
      context.missing(_audioPathMeta);
    }
    if (data.containsKey('created_at')) {
      context.handle(
        _createdAtMeta,
        createdAt.isAcceptableOrUnknown(data['created_at']!, _createdAtMeta),
      );
    }
    if (data.containsKey('retry_count')) {
      context.handle(
        _retryCountMeta,
        retryCount.isAcceptableOrUnknown(data['retry_count']!, _retryCountMeta),
      );
    }
    if (data.containsKey('status')) {
      context.handle(
        _statusMeta,
        status.isAcceptableOrUnknown(data['status']!, _statusMeta),
      );
    }
    if (data.containsKey('error_message')) {
      context.handle(
        _errorMessageMeta,
        errorMessage.isAcceptableOrUnknown(
          data['error_message']!,
          _errorMessageMeta,
        ),
      );
    }
    if (data.containsKey('updated_at')) {
      context.handle(
        _updatedAtMeta,
        updatedAt.isAcceptableOrUnknown(data['updated_at']!, _updatedAtMeta),
      );
    }
    if (data.containsKey('language')) {
      context.handle(
        _languageMeta,
        language.isAcceptableOrUnknown(data['language']!, _languageMeta),
      );
    }
    if (data.containsKey('idempotency_key')) {
      context.handle(
        _idempotencyKeyMeta,
        idempotencyKey.isAcceptableOrUnknown(
          data['idempotency_key']!,
          _idempotencyKeyMeta,
        ),
      );
    }
    return context;
  }

  @override
  Set<GeneratedColumn> get $primaryKey => {id};
  @override
  List<Set<GeneratedColumn>> get uniqueKeys => [
    {audioPath},
  ];
  @override
  PendingUpload map(Map<String, dynamic> data, {String? tablePrefix}) {
    final effectivePrefix = tablePrefix != null ? '$tablePrefix.' : '';
    return PendingUpload(
      id: attachedDatabase.typeMapping.read(
        DriftSqlType.int,
        data['${effectivePrefix}id'],
      )!,
      audioPath: attachedDatabase.typeMapping.read(
        DriftSqlType.string,
        data['${effectivePrefix}audio_path'],
      )!,
      createdAt: attachedDatabase.typeMapping.read(
        DriftSqlType.dateTime,
        data['${effectivePrefix}created_at'],
      )!,
      retryCount: attachedDatabase.typeMapping.read(
        DriftSqlType.int,
        data['${effectivePrefix}retry_count'],
      )!,
      status: attachedDatabase.typeMapping.read(
        DriftSqlType.int,
        data['${effectivePrefix}status'],
      )!,
      errorMessage: attachedDatabase.typeMapping.read(
        DriftSqlType.string,
        data['${effectivePrefix}error_message'],
      ),
      updatedAt: attachedDatabase.typeMapping.read(
        DriftSqlType.dateTime,
        data['${effectivePrefix}updated_at'],
      ),
      language: attachedDatabase.typeMapping.read(
        DriftSqlType.string,
        data['${effectivePrefix}language'],
      ),
      idempotencyKey: attachedDatabase.typeMapping.read(
        DriftSqlType.string,
        data['${effectivePrefix}idempotency_key'],
      ),
    );
  }

  @override
  $PendingUploadsTable createAlias(String alias) {
    return $PendingUploadsTable(attachedDatabase, alias);
  }
}

class PendingUpload extends DataClass implements Insertable<PendingUpload> {
  /// Auto-incrementing primary key.
  final int id;

  /// Path to the audio file on disk.
  final String audioPath;

  /// When this upload was queued.
  final DateTime createdAt;

  /// Number of retry attempts.
  final int retryCount;

  /// Current status.
  final int status;

  /// Optional error message from last failed attempt.
  final String? errorMessage;

  /// When this record was last updated.
  final DateTime? updatedAt;

  /// Optional language hint for transcription.
  final String? language;

  /// Stable idempotency key reused across retries for the same recording.
  final String? idempotencyKey;
  const PendingUpload({
    required this.id,
    required this.audioPath,
    required this.createdAt,
    required this.retryCount,
    required this.status,
    this.errorMessage,
    this.updatedAt,
    this.language,
    this.idempotencyKey,
  });
  @override
  Map<String, Expression> toColumns(bool nullToAbsent) {
    final map = <String, Expression>{};
    map['id'] = Variable<int>(id);
    map['audio_path'] = Variable<String>(audioPath);
    map['created_at'] = Variable<DateTime>(createdAt);
    map['retry_count'] = Variable<int>(retryCount);
    map['status'] = Variable<int>(status);
    if (!nullToAbsent || errorMessage != null) {
      map['error_message'] = Variable<String>(errorMessage);
    }
    if (!nullToAbsent || updatedAt != null) {
      map['updated_at'] = Variable<DateTime>(updatedAt);
    }
    if (!nullToAbsent || language != null) {
      map['language'] = Variable<String>(language);
    }
    if (!nullToAbsent || idempotencyKey != null) {
      map['idempotency_key'] = Variable<String>(idempotencyKey);
    }
    return map;
  }

  PendingUploadsCompanion toCompanion(bool nullToAbsent) {
    return PendingUploadsCompanion(
      id: Value(id),
      audioPath: Value(audioPath),
      createdAt: Value(createdAt),
      retryCount: Value(retryCount),
      status: Value(status),
      errorMessage: errorMessage == null && nullToAbsent
          ? const Value.absent()
          : Value(errorMessage),
      updatedAt: updatedAt == null && nullToAbsent
          ? const Value.absent()
          : Value(updatedAt),
      language: language == null && nullToAbsent
          ? const Value.absent()
          : Value(language),
      idempotencyKey: idempotencyKey == null && nullToAbsent
          ? const Value.absent()
          : Value(idempotencyKey),
    );
  }

  factory PendingUpload.fromJson(
    Map<String, dynamic> json, {
    ValueSerializer? serializer,
  }) {
    serializer ??= driftRuntimeOptions.defaultSerializer;
    return PendingUpload(
      id: serializer.fromJson<int>(json['id']),
      audioPath: serializer.fromJson<String>(json['audioPath']),
      createdAt: serializer.fromJson<DateTime>(json['createdAt']),
      retryCount: serializer.fromJson<int>(json['retryCount']),
      status: serializer.fromJson<int>(json['status']),
      errorMessage: serializer.fromJson<String?>(json['errorMessage']),
      updatedAt: serializer.fromJson<DateTime?>(json['updatedAt']),
      language: serializer.fromJson<String?>(json['language']),
      idempotencyKey: serializer.fromJson<String?>(json['idempotencyKey']),
    );
  }
  @override
  Map<String, dynamic> toJson({ValueSerializer? serializer}) {
    serializer ??= driftRuntimeOptions.defaultSerializer;
    return <String, dynamic>{
      'id': serializer.toJson<int>(id),
      'audioPath': serializer.toJson<String>(audioPath),
      'createdAt': serializer.toJson<DateTime>(createdAt),
      'retryCount': serializer.toJson<int>(retryCount),
      'status': serializer.toJson<int>(status),
      'errorMessage': serializer.toJson<String?>(errorMessage),
      'updatedAt': serializer.toJson<DateTime?>(updatedAt),
      'language': serializer.toJson<String?>(language),
      'idempotencyKey': serializer.toJson<String?>(idempotencyKey),
    };
  }

  PendingUpload copyWith({
    int? id,
    String? audioPath,
    DateTime? createdAt,
    int? retryCount,
    int? status,
    Value<String?> errorMessage = const Value.absent(),
    Value<DateTime?> updatedAt = const Value.absent(),
    Value<String?> language = const Value.absent(),
    Value<String?> idempotencyKey = const Value.absent(),
  }) => PendingUpload(
    id: id ?? this.id,
    audioPath: audioPath ?? this.audioPath,
    createdAt: createdAt ?? this.createdAt,
    retryCount: retryCount ?? this.retryCount,
    status: status ?? this.status,
    errorMessage: errorMessage.present ? errorMessage.value : this.errorMessage,
    updatedAt: updatedAt.present ? updatedAt.value : this.updatedAt,
    language: language.present ? language.value : this.language,
    idempotencyKey: idempotencyKey.present
        ? idempotencyKey.value
        : this.idempotencyKey,
  );
  PendingUpload copyWithCompanion(PendingUploadsCompanion data) {
    return PendingUpload(
      id: data.id.present ? data.id.value : this.id,
      audioPath: data.audioPath.present ? data.audioPath.value : this.audioPath,
      createdAt: data.createdAt.present ? data.createdAt.value : this.createdAt,
      retryCount: data.retryCount.present
          ? data.retryCount.value
          : this.retryCount,
      status: data.status.present ? data.status.value : this.status,
      errorMessage: data.errorMessage.present
          ? data.errorMessage.value
          : this.errorMessage,
      updatedAt: data.updatedAt.present ? data.updatedAt.value : this.updatedAt,
      language: data.language.present ? data.language.value : this.language,
      idempotencyKey: data.idempotencyKey.present
          ? data.idempotencyKey.value
          : this.idempotencyKey,
    );
  }

  @override
  String toString() {
    return (StringBuffer('PendingUpload(')
          ..write('id: $id, ')
          ..write('audioPath: $audioPath, ')
          ..write('createdAt: $createdAt, ')
          ..write('retryCount: $retryCount, ')
          ..write('status: $status, ')
          ..write('errorMessage: $errorMessage, ')
          ..write('updatedAt: $updatedAt, ')
          ..write('language: $language, ')
          ..write('idempotencyKey: $idempotencyKey')
          ..write(')'))
        .toString();
  }

  @override
  int get hashCode => Object.hash(
    id,
    audioPath,
    createdAt,
    retryCount,
    status,
    errorMessage,
    updatedAt,
    language,
    idempotencyKey,
  );
  @override
  bool operator ==(Object other) =>
      identical(this, other) ||
      (other is PendingUpload &&
          other.id == this.id &&
          other.audioPath == this.audioPath &&
          other.createdAt == this.createdAt &&
          other.retryCount == this.retryCount &&
          other.status == this.status &&
          other.errorMessage == this.errorMessage &&
          other.updatedAt == this.updatedAt &&
          other.language == this.language &&
          other.idempotencyKey == this.idempotencyKey);
}

class PendingUploadsCompanion extends UpdateCompanion<PendingUpload> {
  final Value<int> id;
  final Value<String> audioPath;
  final Value<DateTime> createdAt;
  final Value<int> retryCount;
  final Value<int> status;
  final Value<String?> errorMessage;
  final Value<DateTime?> updatedAt;
  final Value<String?> language;
  final Value<String?> idempotencyKey;
  const PendingUploadsCompanion({
    this.id = const Value.absent(),
    this.audioPath = const Value.absent(),
    this.createdAt = const Value.absent(),
    this.retryCount = const Value.absent(),
    this.status = const Value.absent(),
    this.errorMessage = const Value.absent(),
    this.updatedAt = const Value.absent(),
    this.language = const Value.absent(),
    this.idempotencyKey = const Value.absent(),
  });
  PendingUploadsCompanion.insert({
    this.id = const Value.absent(),
    required String audioPath,
    this.createdAt = const Value.absent(),
    this.retryCount = const Value.absent(),
    this.status = const Value.absent(),
    this.errorMessage = const Value.absent(),
    this.updatedAt = const Value.absent(),
    this.language = const Value.absent(),
    this.idempotencyKey = const Value.absent(),
  }) : audioPath = Value(audioPath);
  static Insertable<PendingUpload> custom({
    Expression<int>? id,
    Expression<String>? audioPath,
    Expression<DateTime>? createdAt,
    Expression<int>? retryCount,
    Expression<int>? status,
    Expression<String>? errorMessage,
    Expression<DateTime>? updatedAt,
    Expression<String>? language,
    Expression<String>? idempotencyKey,
  }) {
    return RawValuesInsertable({
      if (id != null) 'id': id,
      if (audioPath != null) 'audio_path': audioPath,
      if (createdAt != null) 'created_at': createdAt,
      if (retryCount != null) 'retry_count': retryCount,
      if (status != null) 'status': status,
      if (errorMessage != null) 'error_message': errorMessage,
      if (updatedAt != null) 'updated_at': updatedAt,
      if (language != null) 'language': language,
      if (idempotencyKey != null) 'idempotency_key': idempotencyKey,
    });
  }

  PendingUploadsCompanion copyWith({
    Value<int>? id,
    Value<String>? audioPath,
    Value<DateTime>? createdAt,
    Value<int>? retryCount,
    Value<int>? status,
    Value<String?>? errorMessage,
    Value<DateTime?>? updatedAt,
    Value<String?>? language,
    Value<String?>? idempotencyKey,
  }) {
    return PendingUploadsCompanion(
      id: id ?? this.id,
      audioPath: audioPath ?? this.audioPath,
      createdAt: createdAt ?? this.createdAt,
      retryCount: retryCount ?? this.retryCount,
      status: status ?? this.status,
      errorMessage: errorMessage ?? this.errorMessage,
      updatedAt: updatedAt ?? this.updatedAt,
      language: language ?? this.language,
      idempotencyKey: idempotencyKey ?? this.idempotencyKey,
    );
  }

  @override
  Map<String, Expression> toColumns(bool nullToAbsent) {
    final map = <String, Expression>{};
    if (id.present) {
      map['id'] = Variable<int>(id.value);
    }
    if (audioPath.present) {
      map['audio_path'] = Variable<String>(audioPath.value);
    }
    if (createdAt.present) {
      map['created_at'] = Variable<DateTime>(createdAt.value);
    }
    if (retryCount.present) {
      map['retry_count'] = Variable<int>(retryCount.value);
    }
    if (status.present) {
      map['status'] = Variable<int>(status.value);
    }
    if (errorMessage.present) {
      map['error_message'] = Variable<String>(errorMessage.value);
    }
    if (updatedAt.present) {
      map['updated_at'] = Variable<DateTime>(updatedAt.value);
    }
    if (language.present) {
      map['language'] = Variable<String>(language.value);
    }
    if (idempotencyKey.present) {
      map['idempotency_key'] = Variable<String>(idempotencyKey.value);
    }
    return map;
  }

  @override
  String toString() {
    return (StringBuffer('PendingUploadsCompanion(')
          ..write('id: $id, ')
          ..write('audioPath: $audioPath, ')
          ..write('createdAt: $createdAt, ')
          ..write('retryCount: $retryCount, ')
          ..write('status: $status, ')
          ..write('errorMessage: $errorMessage, ')
          ..write('updatedAt: $updatedAt, ')
          ..write('language: $language, ')
          ..write('idempotencyKey: $idempotencyKey')
          ..write(')'))
        .toString();
  }
}

class $WebUploadPayloadsTable extends WebUploadPayloads
    with TableInfo<$WebUploadPayloadsTable, WebUploadPayload> {
  @override
  final GeneratedDatabase attachedDatabase;
  final String? _alias;
  $WebUploadPayloadsTable(this.attachedDatabase, [this._alias]);
  static const VerificationMeta _idempotencyKeyMeta = const VerificationMeta(
    'idempotencyKey',
  );
  @override
  late final GeneratedColumn<String> idempotencyKey = GeneratedColumn<String>(
    'idempotency_key',
    aliasedName,
    false,
    type: DriftSqlType.string,
    requiredDuringInsert: true,
  );
  static const VerificationMeta _audioPathMeta = const VerificationMeta(
    'audioPath',
  );
  @override
  late final GeneratedColumn<String> audioPath = GeneratedColumn<String>(
    'audio_path',
    aliasedName,
    false,
    type: DriftSqlType.string,
    requiredDuringInsert: true,
  );
  static const VerificationMeta _audioBytesMeta = const VerificationMeta(
    'audioBytes',
  );
  @override
  late final GeneratedColumn<Uint8List> audioBytes = GeneratedColumn<Uint8List>(
    'audio_bytes',
    aliasedName,
    false,
    type: DriftSqlType.blob,
    requiredDuringInsert: true,
  );
  static const VerificationMeta _filenameMeta = const VerificationMeta(
    'filename',
  );
  @override
  late final GeneratedColumn<String> filename = GeneratedColumn<String>(
    'filename',
    aliasedName,
    false,
    type: DriftSqlType.string,
    requiredDuringInsert: true,
  );
  static const VerificationMeta _contentTypeMeta = const VerificationMeta(
    'contentType',
  );
  @override
  late final GeneratedColumn<String> contentType = GeneratedColumn<String>(
    'content_type',
    aliasedName,
    true,
    type: DriftSqlType.string,
    requiredDuringInsert: false,
  );
  static const VerificationMeta _recordedAtMeta = const VerificationMeta(
    'recordedAt',
  );
  @override
  late final GeneratedColumn<DateTime> recordedAt = GeneratedColumn<DateTime>(
    'recorded_at',
    aliasedName,
    true,
    type: DriftSqlType.dateTime,
    requiredDuringInsert: false,
  );
  static const VerificationMeta _createdAtMeta = const VerificationMeta(
    'createdAt',
  );
  @override
  late final GeneratedColumn<DateTime> createdAt = GeneratedColumn<DateTime>(
    'created_at',
    aliasedName,
    false,
    type: DriftSqlType.dateTime,
    requiredDuringInsert: false,
    defaultValue: currentDateAndTime,
  );
  static const VerificationMeta _updatedAtMeta = const VerificationMeta(
    'updatedAt',
  );
  @override
  late final GeneratedColumn<DateTime> updatedAt = GeneratedColumn<DateTime>(
    'updated_at',
    aliasedName,
    true,
    type: DriftSqlType.dateTime,
    requiredDuringInsert: false,
  );
  @override
  List<GeneratedColumn> get $columns => [
    idempotencyKey,
    audioPath,
    audioBytes,
    filename,
    contentType,
    recordedAt,
    createdAt,
    updatedAt,
  ];
  @override
  String get aliasedName => _alias ?? actualTableName;
  @override
  String get actualTableName => $name;
  static const String $name = 'web_upload_payloads';
  @override
  VerificationContext validateIntegrity(
    Insertable<WebUploadPayload> instance, {
    bool isInserting = false,
  }) {
    final context = VerificationContext();
    final data = instance.toColumns(true);
    if (data.containsKey('idempotency_key')) {
      context.handle(
        _idempotencyKeyMeta,
        idempotencyKey.isAcceptableOrUnknown(
          data['idempotency_key']!,
          _idempotencyKeyMeta,
        ),
      );
    } else if (isInserting) {
      context.missing(_idempotencyKeyMeta);
    }
    if (data.containsKey('audio_path')) {
      context.handle(
        _audioPathMeta,
        audioPath.isAcceptableOrUnknown(data['audio_path']!, _audioPathMeta),
      );
    } else if (isInserting) {
      context.missing(_audioPathMeta);
    }
    if (data.containsKey('audio_bytes')) {
      context.handle(
        _audioBytesMeta,
        audioBytes.isAcceptableOrUnknown(data['audio_bytes']!, _audioBytesMeta),
      );
    } else if (isInserting) {
      context.missing(_audioBytesMeta);
    }
    if (data.containsKey('filename')) {
      context.handle(
        _filenameMeta,
        filename.isAcceptableOrUnknown(data['filename']!, _filenameMeta),
      );
    } else if (isInserting) {
      context.missing(_filenameMeta);
    }
    if (data.containsKey('content_type')) {
      context.handle(
        _contentTypeMeta,
        contentType.isAcceptableOrUnknown(
          data['content_type']!,
          _contentTypeMeta,
        ),
      );
    }
    if (data.containsKey('recorded_at')) {
      context.handle(
        _recordedAtMeta,
        recordedAt.isAcceptableOrUnknown(data['recorded_at']!, _recordedAtMeta),
      );
    }
    if (data.containsKey('created_at')) {
      context.handle(
        _createdAtMeta,
        createdAt.isAcceptableOrUnknown(data['created_at']!, _createdAtMeta),
      );
    }
    if (data.containsKey('updated_at')) {
      context.handle(
        _updatedAtMeta,
        updatedAt.isAcceptableOrUnknown(data['updated_at']!, _updatedAtMeta),
      );
    }
    return context;
  }

  @override
  Set<GeneratedColumn> get $primaryKey => {idempotencyKey};
  @override
  WebUploadPayload map(Map<String, dynamic> data, {String? tablePrefix}) {
    final effectivePrefix = tablePrefix != null ? '$tablePrefix.' : '';
    return WebUploadPayload(
      idempotencyKey: attachedDatabase.typeMapping.read(
        DriftSqlType.string,
        data['${effectivePrefix}idempotency_key'],
      )!,
      audioPath: attachedDatabase.typeMapping.read(
        DriftSqlType.string,
        data['${effectivePrefix}audio_path'],
      )!,
      audioBytes: attachedDatabase.typeMapping.read(
        DriftSqlType.blob,
        data['${effectivePrefix}audio_bytes'],
      )!,
      filename: attachedDatabase.typeMapping.read(
        DriftSqlType.string,
        data['${effectivePrefix}filename'],
      )!,
      contentType: attachedDatabase.typeMapping.read(
        DriftSqlType.string,
        data['${effectivePrefix}content_type'],
      ),
      recordedAt: attachedDatabase.typeMapping.read(
        DriftSqlType.dateTime,
        data['${effectivePrefix}recorded_at'],
      ),
      createdAt: attachedDatabase.typeMapping.read(
        DriftSqlType.dateTime,
        data['${effectivePrefix}created_at'],
      )!,
      updatedAt: attachedDatabase.typeMapping.read(
        DriftSqlType.dateTime,
        data['${effectivePrefix}updated_at'],
      ),
    );
  }

  @override
  $WebUploadPayloadsTable createAlias(String alias) {
    return $WebUploadPayloadsTable(attachedDatabase, alias);
  }
}

class WebUploadPayload extends DataClass
    implements Insertable<WebUploadPayload> {
  /// Stable key shared with the backend idempotency key for this attempt.
  final String idempotencyKey;

  /// Original browser blob URL or durable upload source.
  final String audioPath;

  /// Audio bytes required for chunk replay after a browser reload.
  final Uint8List audioBytes;

  /// Multipart filename derived from the original web recording metadata.
  final String filename;

  /// Optional multipart content type.
  final String? contentType;

  /// Original recording timestamp supplied by the foreground recorder.
  final DateTime? recordedAt;

  /// When this payload was first persisted.
  final DateTime createdAt;

  /// When this payload was last refreshed.
  final DateTime? updatedAt;
  const WebUploadPayload({
    required this.idempotencyKey,
    required this.audioPath,
    required this.audioBytes,
    required this.filename,
    this.contentType,
    this.recordedAt,
    required this.createdAt,
    this.updatedAt,
  });
  @override
  Map<String, Expression> toColumns(bool nullToAbsent) {
    final map = <String, Expression>{};
    map['idempotency_key'] = Variable<String>(idempotencyKey);
    map['audio_path'] = Variable<String>(audioPath);
    map['audio_bytes'] = Variable<Uint8List>(audioBytes);
    map['filename'] = Variable<String>(filename);
    if (!nullToAbsent || contentType != null) {
      map['content_type'] = Variable<String>(contentType);
    }
    if (!nullToAbsent || recordedAt != null) {
      map['recorded_at'] = Variable<DateTime>(recordedAt);
    }
    map['created_at'] = Variable<DateTime>(createdAt);
    if (!nullToAbsent || updatedAt != null) {
      map['updated_at'] = Variable<DateTime>(updatedAt);
    }
    return map;
  }

  WebUploadPayloadsCompanion toCompanion(bool nullToAbsent) {
    return WebUploadPayloadsCompanion(
      idempotencyKey: Value(idempotencyKey),
      audioPath: Value(audioPath),
      audioBytes: Value(audioBytes),
      filename: Value(filename),
      contentType: contentType == null && nullToAbsent
          ? const Value.absent()
          : Value(contentType),
      recordedAt: recordedAt == null && nullToAbsent
          ? const Value.absent()
          : Value(recordedAt),
      createdAt: Value(createdAt),
      updatedAt: updatedAt == null && nullToAbsent
          ? const Value.absent()
          : Value(updatedAt),
    );
  }

  factory WebUploadPayload.fromJson(
    Map<String, dynamic> json, {
    ValueSerializer? serializer,
  }) {
    serializer ??= driftRuntimeOptions.defaultSerializer;
    return WebUploadPayload(
      idempotencyKey: serializer.fromJson<String>(json['idempotencyKey']),
      audioPath: serializer.fromJson<String>(json['audioPath']),
      audioBytes: serializer.fromJson<Uint8List>(json['audioBytes']),
      filename: serializer.fromJson<String>(json['filename']),
      contentType: serializer.fromJson<String?>(json['contentType']),
      recordedAt: serializer.fromJson<DateTime?>(json['recordedAt']),
      createdAt: serializer.fromJson<DateTime>(json['createdAt']),
      updatedAt: serializer.fromJson<DateTime?>(json['updatedAt']),
    );
  }
  @override
  Map<String, dynamic> toJson({ValueSerializer? serializer}) {
    serializer ??= driftRuntimeOptions.defaultSerializer;
    return <String, dynamic>{
      'idempotencyKey': serializer.toJson<String>(idempotencyKey),
      'audioPath': serializer.toJson<String>(audioPath),
      'audioBytes': serializer.toJson<Uint8List>(audioBytes),
      'filename': serializer.toJson<String>(filename),
      'contentType': serializer.toJson<String?>(contentType),
      'recordedAt': serializer.toJson<DateTime?>(recordedAt),
      'createdAt': serializer.toJson<DateTime>(createdAt),
      'updatedAt': serializer.toJson<DateTime?>(updatedAt),
    };
  }

  WebUploadPayload copyWith({
    String? idempotencyKey,
    String? audioPath,
    Uint8List? audioBytes,
    String? filename,
    Value<String?> contentType = const Value.absent(),
    Value<DateTime?> recordedAt = const Value.absent(),
    DateTime? createdAt,
    Value<DateTime?> updatedAt = const Value.absent(),
  }) => WebUploadPayload(
    idempotencyKey: idempotencyKey ?? this.idempotencyKey,
    audioPath: audioPath ?? this.audioPath,
    audioBytes: audioBytes ?? this.audioBytes,
    filename: filename ?? this.filename,
    contentType: contentType.present ? contentType.value : this.contentType,
    recordedAt: recordedAt.present ? recordedAt.value : this.recordedAt,
    createdAt: createdAt ?? this.createdAt,
    updatedAt: updatedAt.present ? updatedAt.value : this.updatedAt,
  );
  WebUploadPayload copyWithCompanion(WebUploadPayloadsCompanion data) {
    return WebUploadPayload(
      idempotencyKey: data.idempotencyKey.present
          ? data.idempotencyKey.value
          : this.idempotencyKey,
      audioPath: data.audioPath.present ? data.audioPath.value : this.audioPath,
      audioBytes: data.audioBytes.present
          ? data.audioBytes.value
          : this.audioBytes,
      filename: data.filename.present ? data.filename.value : this.filename,
      contentType: data.contentType.present
          ? data.contentType.value
          : this.contentType,
      recordedAt: data.recordedAt.present
          ? data.recordedAt.value
          : this.recordedAt,
      createdAt: data.createdAt.present ? data.createdAt.value : this.createdAt,
      updatedAt: data.updatedAt.present ? data.updatedAt.value : this.updatedAt,
    );
  }

  @override
  String toString() {
    return (StringBuffer('WebUploadPayload(')
          ..write('idempotencyKey: $idempotencyKey, ')
          ..write('audioPath: $audioPath, ')
          ..write('audioBytes: $audioBytes, ')
          ..write('filename: $filename, ')
          ..write('contentType: $contentType, ')
          ..write('recordedAt: $recordedAt, ')
          ..write('createdAt: $createdAt, ')
          ..write('updatedAt: $updatedAt')
          ..write(')'))
        .toString();
  }

  @override
  int get hashCode => Object.hash(
    idempotencyKey,
    audioPath,
    $driftBlobEquality.hash(audioBytes),
    filename,
    contentType,
    recordedAt,
    createdAt,
    updatedAt,
  );
  @override
  bool operator ==(Object other) =>
      identical(this, other) ||
      (other is WebUploadPayload &&
          other.idempotencyKey == this.idempotencyKey &&
          other.audioPath == this.audioPath &&
          $driftBlobEquality.equals(other.audioBytes, this.audioBytes) &&
          other.filename == this.filename &&
          other.contentType == this.contentType &&
          other.recordedAt == this.recordedAt &&
          other.createdAt == this.createdAt &&
          other.updatedAt == this.updatedAt);
}

class WebUploadPayloadsCompanion extends UpdateCompanion<WebUploadPayload> {
  final Value<String> idempotencyKey;
  final Value<String> audioPath;
  final Value<Uint8List> audioBytes;
  final Value<String> filename;
  final Value<String?> contentType;
  final Value<DateTime?> recordedAt;
  final Value<DateTime> createdAt;
  final Value<DateTime?> updatedAt;
  final Value<int> rowid;
  const WebUploadPayloadsCompanion({
    this.idempotencyKey = const Value.absent(),
    this.audioPath = const Value.absent(),
    this.audioBytes = const Value.absent(),
    this.filename = const Value.absent(),
    this.contentType = const Value.absent(),
    this.recordedAt = const Value.absent(),
    this.createdAt = const Value.absent(),
    this.updatedAt = const Value.absent(),
    this.rowid = const Value.absent(),
  });
  WebUploadPayloadsCompanion.insert({
    required String idempotencyKey,
    required String audioPath,
    required Uint8List audioBytes,
    required String filename,
    this.contentType = const Value.absent(),
    this.recordedAt = const Value.absent(),
    this.createdAt = const Value.absent(),
    this.updatedAt = const Value.absent(),
    this.rowid = const Value.absent(),
  }) : idempotencyKey = Value(idempotencyKey),
       audioPath = Value(audioPath),
       audioBytes = Value(audioBytes),
       filename = Value(filename);
  static Insertable<WebUploadPayload> custom({
    Expression<String>? idempotencyKey,
    Expression<String>? audioPath,
    Expression<Uint8List>? audioBytes,
    Expression<String>? filename,
    Expression<String>? contentType,
    Expression<DateTime>? recordedAt,
    Expression<DateTime>? createdAt,
    Expression<DateTime>? updatedAt,
    Expression<int>? rowid,
  }) {
    return RawValuesInsertable({
      if (idempotencyKey != null) 'idempotency_key': idempotencyKey,
      if (audioPath != null) 'audio_path': audioPath,
      if (audioBytes != null) 'audio_bytes': audioBytes,
      if (filename != null) 'filename': filename,
      if (contentType != null) 'content_type': contentType,
      if (recordedAt != null) 'recorded_at': recordedAt,
      if (createdAt != null) 'created_at': createdAt,
      if (updatedAt != null) 'updated_at': updatedAt,
      if (rowid != null) 'rowid': rowid,
    });
  }

  WebUploadPayloadsCompanion copyWith({
    Value<String>? idempotencyKey,
    Value<String>? audioPath,
    Value<Uint8List>? audioBytes,
    Value<String>? filename,
    Value<String?>? contentType,
    Value<DateTime?>? recordedAt,
    Value<DateTime>? createdAt,
    Value<DateTime?>? updatedAt,
    Value<int>? rowid,
  }) {
    return WebUploadPayloadsCompanion(
      idempotencyKey: idempotencyKey ?? this.idempotencyKey,
      audioPath: audioPath ?? this.audioPath,
      audioBytes: audioBytes ?? this.audioBytes,
      filename: filename ?? this.filename,
      contentType: contentType ?? this.contentType,
      recordedAt: recordedAt ?? this.recordedAt,
      createdAt: createdAt ?? this.createdAt,
      updatedAt: updatedAt ?? this.updatedAt,
      rowid: rowid ?? this.rowid,
    );
  }

  @override
  Map<String, Expression> toColumns(bool nullToAbsent) {
    final map = <String, Expression>{};
    if (idempotencyKey.present) {
      map['idempotency_key'] = Variable<String>(idempotencyKey.value);
    }
    if (audioPath.present) {
      map['audio_path'] = Variable<String>(audioPath.value);
    }
    if (audioBytes.present) {
      map['audio_bytes'] = Variable<Uint8List>(audioBytes.value);
    }
    if (filename.present) {
      map['filename'] = Variable<String>(filename.value);
    }
    if (contentType.present) {
      map['content_type'] = Variable<String>(contentType.value);
    }
    if (recordedAt.present) {
      map['recorded_at'] = Variable<DateTime>(recordedAt.value);
    }
    if (createdAt.present) {
      map['created_at'] = Variable<DateTime>(createdAt.value);
    }
    if (updatedAt.present) {
      map['updated_at'] = Variable<DateTime>(updatedAt.value);
    }
    if (rowid.present) {
      map['rowid'] = Variable<int>(rowid.value);
    }
    return map;
  }

  @override
  String toString() {
    return (StringBuffer('WebUploadPayloadsCompanion(')
          ..write('idempotencyKey: $idempotencyKey, ')
          ..write('audioPath: $audioPath, ')
          ..write('audioBytes: $audioBytes, ')
          ..write('filename: $filename, ')
          ..write('contentType: $contentType, ')
          ..write('recordedAt: $recordedAt, ')
          ..write('createdAt: $createdAt, ')
          ..write('updatedAt: $updatedAt, ')
          ..write('rowid: $rowid')
          ..write(')'))
        .toString();
  }
}

abstract class _$AppDatabase extends GeneratedDatabase {
  _$AppDatabase(QueryExecutor e) : super(e);
  $AppDatabaseManager get managers => $AppDatabaseManager(this);
  late final $PendingUploadsTable pendingUploads = $PendingUploadsTable(this);
  late final $WebUploadPayloadsTable webUploadPayloads =
      $WebUploadPayloadsTable(this);
  @override
  Iterable<TableInfo<Table, Object?>> get allTables =>
      allSchemaEntities.whereType<TableInfo<Table, Object?>>();
  @override
  List<DatabaseSchemaEntity> get allSchemaEntities => [
    pendingUploads,
    webUploadPayloads,
  ];
}

typedef $$PendingUploadsTableCreateCompanionBuilder =
    PendingUploadsCompanion Function({
      Value<int> id,
      required String audioPath,
      Value<DateTime> createdAt,
      Value<int> retryCount,
      Value<int> status,
      Value<String?> errorMessage,
      Value<DateTime?> updatedAt,
      Value<String?> language,
      Value<String?> idempotencyKey,
    });
typedef $$PendingUploadsTableUpdateCompanionBuilder =
    PendingUploadsCompanion Function({
      Value<int> id,
      Value<String> audioPath,
      Value<DateTime> createdAt,
      Value<int> retryCount,
      Value<int> status,
      Value<String?> errorMessage,
      Value<DateTime?> updatedAt,
      Value<String?> language,
      Value<String?> idempotencyKey,
    });

class $$PendingUploadsTableFilterComposer
    extends Composer<_$AppDatabase, $PendingUploadsTable> {
  $$PendingUploadsTableFilterComposer({
    required super.$db,
    required super.$table,
    super.joinBuilder,
    super.$addJoinBuilderToRootComposer,
    super.$removeJoinBuilderFromRootComposer,
  });
  ColumnFilters<int> get id => $composableBuilder(
    column: $table.id,
    builder: (column) => ColumnFilters(column),
  );

  ColumnFilters<String> get audioPath => $composableBuilder(
    column: $table.audioPath,
    builder: (column) => ColumnFilters(column),
  );

  ColumnFilters<DateTime> get createdAt => $composableBuilder(
    column: $table.createdAt,
    builder: (column) => ColumnFilters(column),
  );

  ColumnFilters<int> get retryCount => $composableBuilder(
    column: $table.retryCount,
    builder: (column) => ColumnFilters(column),
  );

  ColumnFilters<int> get status => $composableBuilder(
    column: $table.status,
    builder: (column) => ColumnFilters(column),
  );

  ColumnFilters<String> get errorMessage => $composableBuilder(
    column: $table.errorMessage,
    builder: (column) => ColumnFilters(column),
  );

  ColumnFilters<DateTime> get updatedAt => $composableBuilder(
    column: $table.updatedAt,
    builder: (column) => ColumnFilters(column),
  );

  ColumnFilters<String> get language => $composableBuilder(
    column: $table.language,
    builder: (column) => ColumnFilters(column),
  );

  ColumnFilters<String> get idempotencyKey => $composableBuilder(
    column: $table.idempotencyKey,
    builder: (column) => ColumnFilters(column),
  );
}

class $$PendingUploadsTableOrderingComposer
    extends Composer<_$AppDatabase, $PendingUploadsTable> {
  $$PendingUploadsTableOrderingComposer({
    required super.$db,
    required super.$table,
    super.joinBuilder,
    super.$addJoinBuilderToRootComposer,
    super.$removeJoinBuilderFromRootComposer,
  });
  ColumnOrderings<int> get id => $composableBuilder(
    column: $table.id,
    builder: (column) => ColumnOrderings(column),
  );

  ColumnOrderings<String> get audioPath => $composableBuilder(
    column: $table.audioPath,
    builder: (column) => ColumnOrderings(column),
  );

  ColumnOrderings<DateTime> get createdAt => $composableBuilder(
    column: $table.createdAt,
    builder: (column) => ColumnOrderings(column),
  );

  ColumnOrderings<int> get retryCount => $composableBuilder(
    column: $table.retryCount,
    builder: (column) => ColumnOrderings(column),
  );

  ColumnOrderings<int> get status => $composableBuilder(
    column: $table.status,
    builder: (column) => ColumnOrderings(column),
  );

  ColumnOrderings<String> get errorMessage => $composableBuilder(
    column: $table.errorMessage,
    builder: (column) => ColumnOrderings(column),
  );

  ColumnOrderings<DateTime> get updatedAt => $composableBuilder(
    column: $table.updatedAt,
    builder: (column) => ColumnOrderings(column),
  );

  ColumnOrderings<String> get language => $composableBuilder(
    column: $table.language,
    builder: (column) => ColumnOrderings(column),
  );

  ColumnOrderings<String> get idempotencyKey => $composableBuilder(
    column: $table.idempotencyKey,
    builder: (column) => ColumnOrderings(column),
  );
}

class $$PendingUploadsTableAnnotationComposer
    extends Composer<_$AppDatabase, $PendingUploadsTable> {
  $$PendingUploadsTableAnnotationComposer({
    required super.$db,
    required super.$table,
    super.joinBuilder,
    super.$addJoinBuilderToRootComposer,
    super.$removeJoinBuilderFromRootComposer,
  });
  GeneratedColumn<int> get id =>
      $composableBuilder(column: $table.id, builder: (column) => column);

  GeneratedColumn<String> get audioPath =>
      $composableBuilder(column: $table.audioPath, builder: (column) => column);

  GeneratedColumn<DateTime> get createdAt =>
      $composableBuilder(column: $table.createdAt, builder: (column) => column);

  GeneratedColumn<int> get retryCount => $composableBuilder(
    column: $table.retryCount,
    builder: (column) => column,
  );

  GeneratedColumn<int> get status =>
      $composableBuilder(column: $table.status, builder: (column) => column);

  GeneratedColumn<String> get errorMessage => $composableBuilder(
    column: $table.errorMessage,
    builder: (column) => column,
  );

  GeneratedColumn<DateTime> get updatedAt =>
      $composableBuilder(column: $table.updatedAt, builder: (column) => column);

  GeneratedColumn<String> get language =>
      $composableBuilder(column: $table.language, builder: (column) => column);

  GeneratedColumn<String> get idempotencyKey => $composableBuilder(
    column: $table.idempotencyKey,
    builder: (column) => column,
  );
}

class $$PendingUploadsTableTableManager
    extends
        RootTableManager<
          _$AppDatabase,
          $PendingUploadsTable,
          PendingUpload,
          $$PendingUploadsTableFilterComposer,
          $$PendingUploadsTableOrderingComposer,
          $$PendingUploadsTableAnnotationComposer,
          $$PendingUploadsTableCreateCompanionBuilder,
          $$PendingUploadsTableUpdateCompanionBuilder,
          (
            PendingUpload,
            BaseReferences<_$AppDatabase, $PendingUploadsTable, PendingUpload>,
          ),
          PendingUpload,
          PrefetchHooks Function()
        > {
  $$PendingUploadsTableTableManager(
    _$AppDatabase db,
    $PendingUploadsTable table,
  ) : super(
        TableManagerState(
          db: db,
          table: table,
          createFilteringComposer: () =>
              $$PendingUploadsTableFilterComposer($db: db, $table: table),
          createOrderingComposer: () =>
              $$PendingUploadsTableOrderingComposer($db: db, $table: table),
          createComputedFieldComposer: () =>
              $$PendingUploadsTableAnnotationComposer($db: db, $table: table),
          updateCompanionCallback:
              ({
                Value<int> id = const Value.absent(),
                Value<String> audioPath = const Value.absent(),
                Value<DateTime> createdAt = const Value.absent(),
                Value<int> retryCount = const Value.absent(),
                Value<int> status = const Value.absent(),
                Value<String?> errorMessage = const Value.absent(),
                Value<DateTime?> updatedAt = const Value.absent(),
                Value<String?> language = const Value.absent(),
                Value<String?> idempotencyKey = const Value.absent(),
              }) => PendingUploadsCompanion(
                id: id,
                audioPath: audioPath,
                createdAt: createdAt,
                retryCount: retryCount,
                status: status,
                errorMessage: errorMessage,
                updatedAt: updatedAt,
                language: language,
                idempotencyKey: idempotencyKey,
              ),
          createCompanionCallback:
              ({
                Value<int> id = const Value.absent(),
                required String audioPath,
                Value<DateTime> createdAt = const Value.absent(),
                Value<int> retryCount = const Value.absent(),
                Value<int> status = const Value.absent(),
                Value<String?> errorMessage = const Value.absent(),
                Value<DateTime?> updatedAt = const Value.absent(),
                Value<String?> language = const Value.absent(),
                Value<String?> idempotencyKey = const Value.absent(),
              }) => PendingUploadsCompanion.insert(
                id: id,
                audioPath: audioPath,
                createdAt: createdAt,
                retryCount: retryCount,
                status: status,
                errorMessage: errorMessage,
                updatedAt: updatedAt,
                language: language,
                idempotencyKey: idempotencyKey,
              ),
          withReferenceMapper: (p0) => p0
              .map((e) => (e.readTable(table), BaseReferences(db, table, e)))
              .toList(),
          prefetchHooksCallback: null,
        ),
      );
}

typedef $$PendingUploadsTableProcessedTableManager =
    ProcessedTableManager<
      _$AppDatabase,
      $PendingUploadsTable,
      PendingUpload,
      $$PendingUploadsTableFilterComposer,
      $$PendingUploadsTableOrderingComposer,
      $$PendingUploadsTableAnnotationComposer,
      $$PendingUploadsTableCreateCompanionBuilder,
      $$PendingUploadsTableUpdateCompanionBuilder,
      (
        PendingUpload,
        BaseReferences<_$AppDatabase, $PendingUploadsTable, PendingUpload>,
      ),
      PendingUpload,
      PrefetchHooks Function()
    >;
typedef $$WebUploadPayloadsTableCreateCompanionBuilder =
    WebUploadPayloadsCompanion Function({
      required String idempotencyKey,
      required String audioPath,
      required Uint8List audioBytes,
      required String filename,
      Value<String?> contentType,
      Value<DateTime?> recordedAt,
      Value<DateTime> createdAt,
      Value<DateTime?> updatedAt,
      Value<int> rowid,
    });
typedef $$WebUploadPayloadsTableUpdateCompanionBuilder =
    WebUploadPayloadsCompanion Function({
      Value<String> idempotencyKey,
      Value<String> audioPath,
      Value<Uint8List> audioBytes,
      Value<String> filename,
      Value<String?> contentType,
      Value<DateTime?> recordedAt,
      Value<DateTime> createdAt,
      Value<DateTime?> updatedAt,
      Value<int> rowid,
    });

class $$WebUploadPayloadsTableFilterComposer
    extends Composer<_$AppDatabase, $WebUploadPayloadsTable> {
  $$WebUploadPayloadsTableFilterComposer({
    required super.$db,
    required super.$table,
    super.joinBuilder,
    super.$addJoinBuilderToRootComposer,
    super.$removeJoinBuilderFromRootComposer,
  });
  ColumnFilters<String> get idempotencyKey => $composableBuilder(
    column: $table.idempotencyKey,
    builder: (column) => ColumnFilters(column),
  );

  ColumnFilters<String> get audioPath => $composableBuilder(
    column: $table.audioPath,
    builder: (column) => ColumnFilters(column),
  );

  ColumnFilters<Uint8List> get audioBytes => $composableBuilder(
    column: $table.audioBytes,
    builder: (column) => ColumnFilters(column),
  );

  ColumnFilters<String> get filename => $composableBuilder(
    column: $table.filename,
    builder: (column) => ColumnFilters(column),
  );

  ColumnFilters<String> get contentType => $composableBuilder(
    column: $table.contentType,
    builder: (column) => ColumnFilters(column),
  );

  ColumnFilters<DateTime> get recordedAt => $composableBuilder(
    column: $table.recordedAt,
    builder: (column) => ColumnFilters(column),
  );

  ColumnFilters<DateTime> get createdAt => $composableBuilder(
    column: $table.createdAt,
    builder: (column) => ColumnFilters(column),
  );

  ColumnFilters<DateTime> get updatedAt => $composableBuilder(
    column: $table.updatedAt,
    builder: (column) => ColumnFilters(column),
  );
}

class $$WebUploadPayloadsTableOrderingComposer
    extends Composer<_$AppDatabase, $WebUploadPayloadsTable> {
  $$WebUploadPayloadsTableOrderingComposer({
    required super.$db,
    required super.$table,
    super.joinBuilder,
    super.$addJoinBuilderToRootComposer,
    super.$removeJoinBuilderFromRootComposer,
  });
  ColumnOrderings<String> get idempotencyKey => $composableBuilder(
    column: $table.idempotencyKey,
    builder: (column) => ColumnOrderings(column),
  );

  ColumnOrderings<String> get audioPath => $composableBuilder(
    column: $table.audioPath,
    builder: (column) => ColumnOrderings(column),
  );

  ColumnOrderings<Uint8List> get audioBytes => $composableBuilder(
    column: $table.audioBytes,
    builder: (column) => ColumnOrderings(column),
  );

  ColumnOrderings<String> get filename => $composableBuilder(
    column: $table.filename,
    builder: (column) => ColumnOrderings(column),
  );

  ColumnOrderings<String> get contentType => $composableBuilder(
    column: $table.contentType,
    builder: (column) => ColumnOrderings(column),
  );

  ColumnOrderings<DateTime> get recordedAt => $composableBuilder(
    column: $table.recordedAt,
    builder: (column) => ColumnOrderings(column),
  );

  ColumnOrderings<DateTime> get createdAt => $composableBuilder(
    column: $table.createdAt,
    builder: (column) => ColumnOrderings(column),
  );

  ColumnOrderings<DateTime> get updatedAt => $composableBuilder(
    column: $table.updatedAt,
    builder: (column) => ColumnOrderings(column),
  );
}

class $$WebUploadPayloadsTableAnnotationComposer
    extends Composer<_$AppDatabase, $WebUploadPayloadsTable> {
  $$WebUploadPayloadsTableAnnotationComposer({
    required super.$db,
    required super.$table,
    super.joinBuilder,
    super.$addJoinBuilderToRootComposer,
    super.$removeJoinBuilderFromRootComposer,
  });
  GeneratedColumn<String> get idempotencyKey => $composableBuilder(
    column: $table.idempotencyKey,
    builder: (column) => column,
  );

  GeneratedColumn<String> get audioPath =>
      $composableBuilder(column: $table.audioPath, builder: (column) => column);

  GeneratedColumn<Uint8List> get audioBytes => $composableBuilder(
    column: $table.audioBytes,
    builder: (column) => column,
  );

  GeneratedColumn<String> get filename =>
      $composableBuilder(column: $table.filename, builder: (column) => column);

  GeneratedColumn<String> get contentType => $composableBuilder(
    column: $table.contentType,
    builder: (column) => column,
  );

  GeneratedColumn<DateTime> get recordedAt => $composableBuilder(
    column: $table.recordedAt,
    builder: (column) => column,
  );

  GeneratedColumn<DateTime> get createdAt =>
      $composableBuilder(column: $table.createdAt, builder: (column) => column);

  GeneratedColumn<DateTime> get updatedAt =>
      $composableBuilder(column: $table.updatedAt, builder: (column) => column);
}

class $$WebUploadPayloadsTableTableManager
    extends
        RootTableManager<
          _$AppDatabase,
          $WebUploadPayloadsTable,
          WebUploadPayload,
          $$WebUploadPayloadsTableFilterComposer,
          $$WebUploadPayloadsTableOrderingComposer,
          $$WebUploadPayloadsTableAnnotationComposer,
          $$WebUploadPayloadsTableCreateCompanionBuilder,
          $$WebUploadPayloadsTableUpdateCompanionBuilder,
          (
            WebUploadPayload,
            BaseReferences<
              _$AppDatabase,
              $WebUploadPayloadsTable,
              WebUploadPayload
            >,
          ),
          WebUploadPayload,
          PrefetchHooks Function()
        > {
  $$WebUploadPayloadsTableTableManager(
    _$AppDatabase db,
    $WebUploadPayloadsTable table,
  ) : super(
        TableManagerState(
          db: db,
          table: table,
          createFilteringComposer: () =>
              $$WebUploadPayloadsTableFilterComposer($db: db, $table: table),
          createOrderingComposer: () =>
              $$WebUploadPayloadsTableOrderingComposer($db: db, $table: table),
          createComputedFieldComposer: () =>
              $$WebUploadPayloadsTableAnnotationComposer(
                $db: db,
                $table: table,
              ),
          updateCompanionCallback:
              ({
                Value<String> idempotencyKey = const Value.absent(),
                Value<String> audioPath = const Value.absent(),
                Value<Uint8List> audioBytes = const Value.absent(),
                Value<String> filename = const Value.absent(),
                Value<String?> contentType = const Value.absent(),
                Value<DateTime?> recordedAt = const Value.absent(),
                Value<DateTime> createdAt = const Value.absent(),
                Value<DateTime?> updatedAt = const Value.absent(),
                Value<int> rowid = const Value.absent(),
              }) => WebUploadPayloadsCompanion(
                idempotencyKey: idempotencyKey,
                audioPath: audioPath,
                audioBytes: audioBytes,
                filename: filename,
                contentType: contentType,
                recordedAt: recordedAt,
                createdAt: createdAt,
                updatedAt: updatedAt,
                rowid: rowid,
              ),
          createCompanionCallback:
              ({
                required String idempotencyKey,
                required String audioPath,
                required Uint8List audioBytes,
                required String filename,
                Value<String?> contentType = const Value.absent(),
                Value<DateTime?> recordedAt = const Value.absent(),
                Value<DateTime> createdAt = const Value.absent(),
                Value<DateTime?> updatedAt = const Value.absent(),
                Value<int> rowid = const Value.absent(),
              }) => WebUploadPayloadsCompanion.insert(
                idempotencyKey: idempotencyKey,
                audioPath: audioPath,
                audioBytes: audioBytes,
                filename: filename,
                contentType: contentType,
                recordedAt: recordedAt,
                createdAt: createdAt,
                updatedAt: updatedAt,
                rowid: rowid,
              ),
          withReferenceMapper: (p0) => p0
              .map((e) => (e.readTable(table), BaseReferences(db, table, e)))
              .toList(),
          prefetchHooksCallback: null,
        ),
      );
}

typedef $$WebUploadPayloadsTableProcessedTableManager =
    ProcessedTableManager<
      _$AppDatabase,
      $WebUploadPayloadsTable,
      WebUploadPayload,
      $$WebUploadPayloadsTableFilterComposer,
      $$WebUploadPayloadsTableOrderingComposer,
      $$WebUploadPayloadsTableAnnotationComposer,
      $$WebUploadPayloadsTableCreateCompanionBuilder,
      $$WebUploadPayloadsTableUpdateCompanionBuilder,
      (
        WebUploadPayload,
        BaseReferences<
          _$AppDatabase,
          $WebUploadPayloadsTable,
          WebUploadPayload
        >,
      ),
      WebUploadPayload,
      PrefetchHooks Function()
    >;

class $AppDatabaseManager {
  final _$AppDatabase _db;
  $AppDatabaseManager(this._db);
  $$PendingUploadsTableTableManager get pendingUploads =>
      $$PendingUploadsTableTableManager(_db, _db.pendingUploads);
  $$WebUploadPayloadsTableTableManager get webUploadPayloads =>
      $$WebUploadPayloadsTableTableManager(_db, _db.webUploadPayloads);
}
