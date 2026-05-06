import 'dart:io';

import 'android_release_sqlite_verifier.dart';

Future<void> main(List<String> args) async {
  if (args.length != 1) {
    stderr.writeln(
      'Usage: dart run tool/verify_android_release_sqlite.dart <apk-path>',
    );
    exitCode = 64;
    return;
  }

  try {
    await verifyAndroidReleaseSqlite(File(args.single));
    stdout.writeln('Android release APK includes SQLite for every native ABI.');
  } on AndroidReleaseSqliteVerificationException catch (error) {
    stderr.writeln(error.message);
    exitCode = 1;
  }
}
