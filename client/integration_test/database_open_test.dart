import 'package:flutter_test/flutter_test.dart';
import 'package:integration_test/integration_test.dart';
import 'package:oracy/db/database.dart';

void main() {
  IntegrationTestWidgetsFlutterBinding.ensureInitialized();

  testWidgets(
    'Given the Android app opens its production database, When SQLite is queried, Then native loading succeeds',
    (tester) async {
      final db = AppDatabase();
      addTearDown(db.close);

      final rows = await db
          .customSelect('select sqlite_version() as version')
          .get();

      expect(rows.single.read<String>('version'), isNotEmpty);
    },
  );
}
