import 'package:flutter/services.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:oracy/services/home_widget_service.dart';

void main() {
  TestWidgetsFlutterBinding.ensureInitialized();

  const homeWidgetChannel = MethodChannel('home_widget');
  const homeWidgetUpdatesChannel = MethodChannel('home_widget/updates');
  const oracyWidgetChannel = MethodChannel('app.oracy.oracy/widget');

  Future<void> sendWidgetRecordRequest() async {
    final messenger =
        TestDefaultBinaryMessengerBinding.instance.defaultBinaryMessenger;
    await messenger.handlePlatformMessage(
      oracyWidgetChannel.name,
      oracyWidgetChannel.codec.encodeMethodCall(
        const MethodCall('startRecordingFromWidget'),
      ),
      (ByteData? _) {},
    );
  }

  setUp(() {
    final messenger =
        TestDefaultBinaryMessengerBinding.instance.defaultBinaryMessenger;
    messenger.setMockMethodCallHandler(homeWidgetChannel, (_) async => true);
    messenger.setMockMethodCallHandler(
      homeWidgetUpdatesChannel,
      (_) async => null,
    );
    messenger.setMockMethodCallHandler(oracyWidgetChannel, (call) async {
      if (call.method == 'consumePendingRecordIntent') {
        return false;
      }
      return null;
    });
    HomeWidgetService.setOnRecordCallback(null);
  });

  tearDown(() {
    final messenger =
        TestDefaultBinaryMessengerBinding.instance.defaultBinaryMessenger;
    messenger.setMockMethodCallHandler(homeWidgetChannel, null);
    messenger.setMockMethodCallHandler(homeWidgetUpdatesChannel, null);
    messenger.setMockMethodCallHandler(oracyWidgetChannel, null);
    HomeWidgetService.setOnRecordCallback(null);
  });

  test(
    'Given a widget record request arrives before the Dart callback is registered, When the callback is registered, Then recording starts exactly once',
    () async {
      var callbackCount = 0;

      await HomeWidgetService.initialize();
      await sendWidgetRecordRequest();
      HomeWidgetService.setOnRecordCallback(() => callbackCount++);
      HomeWidgetService.setOnRecordCallback(() => callbackCount++);

      expect(callbackCount, 1);
    },
  );
}
