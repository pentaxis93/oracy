import 'dart:io';

import 'package:archive/archive_io.dart';
import 'package:flutter_test/flutter_test.dart';

import '../tool/android_release_sqlite_verifier.dart';

void main() {
  test(
    'Given an APK ships native app code for an ABI without SQLite, When Android release SQLite packaging is verified, Then the missing ABI is rejected',
    () async {
      final apk = await _writeApk({
        'lib/arm64-v8a/libapp.so': [1],
      });

      expect(
        () => verifyAndroidReleaseSqlite(apk),
        throwsA(
          isA<AndroidReleaseSqliteVerificationException>().having(
            (error) => error.message,
            'message',
            contains('arm64-v8a'),
          ),
        ),
      );
    },
  );

  test(
    'Given an APK ships SQLite for every native app ABI, When Android release SQLite packaging is verified, Then verification succeeds',
    () async {
      final apk = await _writeApk({
        'lib/arm64-v8a/libapp.so': [1],
        'lib/arm64-v8a/libsqlite3.arm64.android.so': [2],
        'lib/x86_64/libapp.so': [3],
        'lib/x86_64/libsqlite3.x64.android.so': [4],
      });

      expect(verifyAndroidReleaseSqlite(apk), completes);
    },
  );
}

Future<File> _writeApk(Map<String, List<int>> entries) async {
  final tempDir = await Directory.systemTemp.createTemp('oracy-apk-test-');
  addTearDown(() async {
    if (await tempDir.exists()) {
      await tempDir.delete(recursive: true);
    }
  });

  final apk = File('${tempDir.path}/app-release.apk');
  final encoder = ZipFileEncoder()..create(apk.path);
  for (final entry in entries.entries) {
    encoder.addArchiveFile(ArchiveFile.bytes(entry.key, entry.value));
  }
  encoder.closeSync();
  return apk;
}
