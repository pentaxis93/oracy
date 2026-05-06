import 'dart:io';

import 'package:archive/archive.dart';

class AndroidReleaseSqliteVerificationException implements Exception {
  AndroidReleaseSqliteVerificationException(this.message);

  final String message;

  @override
  String toString() => message;
}

Future<void> verifyAndroidReleaseSqlite(File apk) async {
  if (!await apk.exists()) {
    throw AndroidReleaseSqliteVerificationException(
      'Android release APK does not exist: ${apk.path}',
    );
  }

  final archive = ZipDecoder().decodeBytes(await apk.readAsBytes());
  final entryNames = archive.files
      .where((file) => !file.isDirectory)
      .map((file) => file.name)
      .toSet();

  final appAbis = _abisMatching(
    entryNames,
    RegExp(r'^lib/([^/]+)/libapp\.so$'),
  );
  final sqliteAbis = _abisMatching(
    entryNames,
    RegExp(r'^lib/([^/]+)/libsqlite3(?:[._-].*)?\.so$'),
  );

  if (appAbis.isEmpty) {
    throw AndroidReleaseSqliteVerificationException(
      'Android release APK has no native libapp.so entries.',
    );
  }

  final missingSqliteAbis = appAbis.difference(sqliteAbis).toList()..sort();
  if (missingSqliteAbis.isNotEmpty) {
    throw AndroidReleaseSqliteVerificationException(
      'Android release APK is missing SQLite native libraries for: '
      '${missingSqliteAbis.join(', ')}',
    );
  }
}

Set<String> _abisMatching(Set<String> entryNames, RegExp pattern) {
  return {
    for (final name in entryNames)
      if (pattern.firstMatch(name) case final match?) match.group(1)!,
  };
}
