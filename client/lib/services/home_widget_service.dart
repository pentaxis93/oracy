import 'package:flutter/foundation.dart';
import 'package:flutter/services.dart';
import 'package:home_widget/home_widget.dart';

/// Callback type for widget-triggered recording.
typedef OnWidgetRecordCallback = void Function();

/// Service for managing the Oracy home screen widget.
///
/// Handles:
/// - Updating widget data from Flutter
/// - Receiving widget interactions
/// - Triggering widget updates
class HomeWidgetService {
  /// The app group ID for iOS (not used for Android).
  static const String _appGroupId = 'group.app.oracy.oracy';

  /// The widget name for Android.
  static const String _androidWidgetName = 'OracyWidgetProvider';

  /// Method channel for communication with native code.
  static const MethodChannel _channel = MethodChannel('app.oracy.oracy/widget');

  /// Callback when widget triggers recording.
  static OnWidgetRecordCallback? _onRecordCallback;

  /// Initialize the home widget service.
  static Future<void> initialize() async {
    // Set the app group ID for iOS
    await HomeWidget.setAppGroupId(_appGroupId);

    // Register callback for widget interactions
    HomeWidget.widgetClicked.listen(_handleWidgetClick);

    // Set up method channel handler
    _channel.setMethodCallHandler(_handleMethodCall);

    debugPrint('Home widget: Initialized');
  }

  /// Set the callback for when the widget triggers recording.
  static void setOnRecordCallback(OnWidgetRecordCallback? callback) {
    _onRecordCallback = callback;
  }

  /// Handle method calls from native code.
  static Future<dynamic> _handleMethodCall(MethodCall call) async {
    switch (call.method) {
      case 'startRecordingFromWidget':
        debugPrint('Home widget: Received startRecordingFromWidget');
        _onRecordCallback?.call();
        return true;
      default:
        throw PlatformException(
          code: 'NOT_IMPLEMENTED',
          message: 'Method ${call.method} not implemented',
        );
    }
  }

  /// Handle widget click events.
  static void _handleWidgetClick(Uri? uri) {
    debugPrint('Home widget: Clicked with URI: $uri');
    // The widget click will open the app via the PendingIntent
    // Additional handling can be added here if needed
  }

  /// Update the widget status text.
  static Future<void> updateStatus(String status) async {
    // Skip on web - home widgets not supported
    if (kIsWeb) return;

    await HomeWidget.saveWidgetData('status', status);
    await updateWidget();
  }

  /// Trigger a widget update on Android.
  static Future<void> updateWidget() async {
    // Skip on web - home widgets not supported
    if (kIsWeb) return;

    try {
      await HomeWidget.updateWidget(androidName: _androidWidgetName);
      debugPrint('Home widget: Updated');
    } catch (e) {
      debugPrint('Home widget: Failed to update - $e');
    }
  }

  /// Check if home widgets are supported on this device.
  static Future<bool> isSupported() async {
    // home_widget is not supported on web
    if (kIsWeb) return false;

    // home_widget is always supported on Android
    return true;
  }
}
