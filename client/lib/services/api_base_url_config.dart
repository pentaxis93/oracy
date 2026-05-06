/// Default API base URL.
const kDefaultBaseUrl = String.fromEnvironment(
  'ORACY_API_BASE_URL',
  defaultValue: 'https://api.oracy.app',
);

/// Normalize and validate an operator-provided API base URL.
String normalizeApiBaseUrl(String value) {
  final uri = Uri.tryParse(value.trim());
  if (uri == null || !uri.hasScheme) {
    throw const FormatException('Enter an absolute http or https URL.');
  }

  final scheme = uri.scheme.toLowerCase();
  if (scheme != 'http' && scheme != 'https') {
    throw const FormatException('Server URL must use http or https.');
  }

  if (uri.host.isEmpty) {
    throw const FormatException('Server URL must include a host.');
  }

  if (uri.userInfo.isNotEmpty) {
    throw const FormatException('Server URL must not include credentials.');
  }

  if (uri.hasQuery || uri.hasFragment) {
    throw const FormatException(
      'Server URL must not include query parameters or fragments.',
    );
  }

  final hasPath = uri.pathSegments.any((segment) => segment.isNotEmpty);
  if (hasPath) {
    throw const FormatException('Server URL must not include a path.');
  }

  return uri
      .replace(scheme: scheme, host: uri.host.toLowerCase(), path: '')
      .toString();
}
