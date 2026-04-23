import 'dart:io';

import 'package:flutter/foundation.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';
import 'package:permission_handler/permission_handler.dart';

/// Permission status for microphone access.
enum MicrophonePermissionStatus {
  /// Permission has been granted.
  granted,

  /// Permission has been denied (can request again).
  denied,

  /// Permission has been permanently denied (must open settings).
  permanentlyDenied,

  /// Permission is restricted (iOS only).
  restricted,

  /// Permission status is unknown.
  unknown,
}

/// Service for handling microphone permission requests.
class PermissionService {
  /// Check the current microphone permission status.
  Future<MicrophonePermissionStatus> checkMicrophonePermission() async {
    // On web, we can't check permission status without requesting it
    // so we'll assume it needs to be granted through the browser
    if (kIsWeb) {
      return MicrophonePermissionStatus.granted;
    }

    final status = await Permission.microphone.status;
    return _mapStatus(status);
  }

  /// Request microphone permission.
  ///
  /// Returns the new permission status after the request.
  Future<MicrophonePermissionStatus> requestMicrophonePermission() async {
    // On web, permissions are handled by the browser when accessing media
    // Return granted so the dialog closes and we let the browser handle it
    if (kIsWeb) {
      return MicrophonePermissionStatus.granted;
    }

    final status = await Permission.microphone.request();
    return _mapStatus(status);
  }

  /// Open app settings for the user to grant permission manually.
  ///
  /// Returns true if settings were opened successfully.
  Future<bool> openSettings() async {
    if (kIsWeb) {
      // Can't open settings on web
      return false;
    }
    return await openAppSettings();
  }

  /// Check if microphone permission is currently granted.
  Future<bool> isMicrophoneGranted() async {
    final status = await checkMicrophonePermission();
    return status == MicrophonePermissionStatus.granted;
  }

  /// Request notification permission (required for Android 13+).
  ///
  /// On Android 13+, apps need explicit permission to post notifications.
  /// This is required for the foreground service notification during recording.
  /// On older Android versions and other platforms, this returns true.
  Future<bool> requestNotificationPermission() async {
    if (kIsWeb) return true;
    if (!Platform.isAndroid) return true;

    // Check if notification permission is already granted
    final status = await Permission.notification.status;
    if (status.isGranted) return true;

    // Request permission
    final result = await Permission.notification.request();
    return result.isGranted;
  }

  /// Ensure both microphone and notification permissions are granted.
  ///
  /// Returns true if all required permissions are granted.
  /// This should be called before starting recording on Android.
  Future<bool> ensureRecordingPermissions() async {
    final micGranted = await isMicrophoneGranted();
    if (!micGranted) return false;

    // On Android 13+, we also need notification permission for foreground service
    final notifGranted = await requestNotificationPermission();
    return notifGranted;
  }

  MicrophonePermissionStatus _mapStatus(PermissionStatus status) {
    switch (status) {
      case PermissionStatus.granted:
      case PermissionStatus.limited:
        return MicrophonePermissionStatus.granted;
      case PermissionStatus.denied:
        return MicrophonePermissionStatus.denied;
      case PermissionStatus.permanentlyDenied:
        return MicrophonePermissionStatus.permanentlyDenied;
      case PermissionStatus.restricted:
        return MicrophonePermissionStatus.restricted;
      case PermissionStatus.provisional:
        return MicrophonePermissionStatus.granted;
    }
  }
}

/// Provider for the permission service.
final permissionServiceProvider = Provider<PermissionService>((ref) {
  return PermissionService();
});

/// Provider for the current microphone permission status.
///
/// This is a FutureProvider that fetches the current status.
/// Invalidate this provider to re-check permission status.
final microphonePermissionProvider = FutureProvider<MicrophonePermissionStatus>(
  (ref) async {
    final service = ref.watch(permissionServiceProvider);
    return service.checkMicrophonePermission();
  },
);
