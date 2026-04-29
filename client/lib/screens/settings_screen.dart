import 'dart:async';

import 'package:flutter/material.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';
import 'package:oracy/services/api_client.dart';
import 'package:oracy/services/preferences_service.dart';
import 'package:oracy/services/upload_queue_service.dart';

/// Provider for the current API key value (for display purposes).
/// Returns masked version if key exists, null otherwise.
final apiKeyDisplayProvider = FutureProvider<String?>((ref) async {
  final storage = ref.watch(secureStorageProvider);
  final key = await storage.getApiKey();
  if (key == null || key.isEmpty) return null;
  // Show first 4 and last 4 chars, mask the rest
  if (key.length <= 8) return '****';
  return '${key.substring(0, 4)}${'*' * (key.length - 8)}${key.substring(key.length - 4)}';
});

/// Settings screen for configuring API key and server URL.
class SettingsScreen extends ConsumerStatefulWidget {
  const SettingsScreen({super.key});

  @override
  ConsumerState<SettingsScreen> createState() => _SettingsScreenState();
}

class _SettingsScreenState extends ConsumerState<SettingsScreen> {
  final _apiKeyController = TextEditingController();
  final _formKey = GlobalKey<FormState>();
  bool _isLoading = false;
  bool _obscureApiKey = true;
  bool _hasExistingKey = false;

  @override
  void initState() {
    super.initState();
    _loadExistingKey();
  }

  Future<void> _loadExistingKey() async {
    final storage = ref.read(secureStorageProvider);
    final hasKey = await storage.hasApiKey();
    if (mounted) {
      setState(() {
        _hasExistingKey = hasKey;
      });
    }
  }

  @override
  void dispose() {
    _apiKeyController.dispose();
    super.dispose();
  }

  Future<void> _saveApiKey() async {
    if (!_formKey.currentState!.validate()) return;

    setState(() => _isLoading = true);

    try {
      final storage = ref.read(secureStorageProvider);
      await storage.setApiKey(_apiKeyController.text.trim());

      // Invalidate the hasApiKey provider to refresh state
      ref.invalidate(hasApiKeyProvider);
      ref.invalidate(apiKeyDisplayProvider);
      unawaited(ref.read(uploadQueueServiceProvider)?.processQueue());

      if (mounted) {
        setState(() {
          _hasExistingKey = true;
          _apiKeyController.clear();
        });
        ScaffoldMessenger.of(context).showSnackBar(
          const SnackBar(
            content: Text('API key saved successfully'),
            backgroundColor: Colors.green,
          ),
        );
      }
    } catch (e) {
      if (mounted) {
        ScaffoldMessenger.of(context).showSnackBar(
          SnackBar(
            content: Text('Failed to save API key: $e'),
            backgroundColor: Colors.red,
          ),
        );
      }
    } finally {
      if (mounted) {
        setState(() => _isLoading = false);
      }
    }
  }

  Future<void> _deleteApiKey() async {
    final confirmed = await showDialog<bool>(
      context: context,
      builder: (context) => AlertDialog(
        title: const Text('Delete API Key'),
        content: const Text(
          'Are you sure you want to delete your API key? '
          'You will need to enter it again to use the app.',
        ),
        actions: [
          TextButton(
            onPressed: () => Navigator.pop(context, false),
            child: const Text('Cancel'),
          ),
          TextButton(
            onPressed: () => Navigator.pop(context, true),
            style: TextButton.styleFrom(foregroundColor: Colors.red),
            child: const Text('Delete'),
          ),
        ],
      ),
    );

    if (confirmed != true) return;

    setState(() => _isLoading = true);

    try {
      final storage = ref.read(secureStorageProvider);
      await storage.deleteApiKey();

      ref.invalidate(hasApiKeyProvider);
      ref.invalidate(apiKeyDisplayProvider);

      if (mounted) {
        setState(() => _hasExistingKey = false);
        ScaffoldMessenger.of(
          context,
        ).showSnackBar(const SnackBar(content: Text('API key deleted')));
      }
    } catch (e) {
      if (mounted) {
        ScaffoldMessenger.of(context).showSnackBar(
          SnackBar(
            content: Text('Failed to delete API key: $e'),
            backgroundColor: Colors.red,
          ),
        );
      }
    } finally {
      if (mounted) {
        setState(() => _isLoading = false);
      }
    }
  }

  @override
  Widget build(BuildContext context) {
    final theme = Theme.of(context);
    final apiKeyDisplay = ref.watch(apiKeyDisplayProvider);

    return Scaffold(
      appBar: AppBar(title: const Text('Settings'), centerTitle: true),
      body: SingleChildScrollView(
        padding: const EdgeInsets.all(16),
        child: Form(
          key: _formKey,
          child: Column(
            crossAxisAlignment: CrossAxisAlignment.start,
            children: [
              // API Configuration Section
              Text(
                'API Configuration',
                style: theme.textTheme.titleMedium?.copyWith(
                  fontWeight: FontWeight.bold,
                ),
              ),
              const SizedBox(height: 8),
              Text(
                'Enter your Oracy API key to enable transcription.',
                style: theme.textTheme.bodyMedium?.copyWith(
                  color: theme.colorScheme.onSurfaceVariant,
                ),
              ),
              const SizedBox(height: 16),

              // Current API Key Status
              if (_hasExistingKey) ...[
                Card(
                  child: ListTile(
                    leading: Icon(
                      Icons.check_circle,
                      color: theme.colorScheme.primary,
                    ),
                    title: const Text('API Key Configured'),
                    subtitle: apiKeyDisplay.when(
                      data: (masked) => Text(masked ?? 'Key stored'),
                      loading: () => const Text('Loading...'),
                      error: (e, s) => const Text('Key stored'),
                    ),
                    trailing: IconButton(
                      icon: const Icon(Icons.delete_outline),
                      onPressed: _isLoading ? null : _deleteApiKey,
                      tooltip: 'Delete API key',
                    ),
                  ),
                ),
                const SizedBox(height: 16),
                Text('Update API Key', style: theme.textTheme.titleSmall),
                const SizedBox(height: 8),
              ],

              // API Key Input
              TextFormField(
                controller: _apiKeyController,
                obscureText: _obscureApiKey,
                decoration: InputDecoration(
                  labelText: _hasExistingKey ? 'New API Key' : 'API Key',
                  hintText: 'Enter your API key',
                  border: const OutlineInputBorder(),
                  prefixIcon: const Icon(Icons.key),
                  suffixIcon: IconButton(
                    icon: Icon(
                      _obscureApiKey ? Icons.visibility : Icons.visibility_off,
                    ),
                    onPressed: () {
                      setState(() => _obscureApiKey = !_obscureApiKey);
                    },
                    tooltip: _obscureApiKey ? 'Show API key' : 'Hide API key',
                  ),
                ),
                validator: (value) {
                  if (value == null || value.trim().isEmpty) {
                    return 'Please enter your API key';
                  }
                  if (value.trim().length < 8) {
                    return 'API key seems too short';
                  }
                  return null;
                },
                onFieldSubmitted: (_) => _saveApiKey(),
              ),
              const SizedBox(height: 16),

              // Save Button
              SizedBox(
                width: double.infinity,
                child: FilledButton.icon(
                  onPressed: _isLoading ? null : _saveApiKey,
                  icon: _isLoading
                      ? const SizedBox(
                          width: 20,
                          height: 20,
                          child: CircularProgressIndicator(strokeWidth: 2),
                        )
                      : const Icon(Icons.save),
                  label: Text(
                    _hasExistingKey ? 'Update API Key' : 'Save API Key',
                  ),
                ),
              ),

              const SizedBox(height: 32),

              // Behavior Section
              Text(
                'Behavior',
                style: theme.textTheme.titleMedium?.copyWith(
                  fontWeight: FontWeight.bold,
                ),
              ),
              const SizedBox(height: 8),
              Card(child: _AutoCopyToggle()),

              const SizedBox(height: 32),

              // Server URL Section (informational for now)
              Text(
                'Server',
                style: theme.textTheme.titleMedium?.copyWith(
                  fontWeight: FontWeight.bold,
                ),
              ),
              const SizedBox(height: 8),
              Card(
                child: ListTile(
                  leading: const Icon(Icons.dns),
                  title: const Text('Server URL'),
                  subtitle: const Text(kDefaultBaseUrl),
                ),
              ),

              const SizedBox(height: 32),

              // About Section
              Text(
                'About',
                style: theme.textTheme.titleMedium?.copyWith(
                  fontWeight: FontWeight.bold,
                ),
              ),
              const SizedBox(height: 8),
              Card(
                child: Column(
                  children: [
                    ListTile(
                      leading: const Icon(Icons.info_outline),
                      title: const Text('Oracy'),
                      subtitle: const Text('Voice transcription made simple'),
                    ),
                    const Divider(height: 1),
                    ListTile(
                      leading: const Icon(Icons.code),
                      title: const Text('Version'),
                      subtitle: const Text('1.0.0'),
                    ),
                  ],
                ),
              ),
            ],
          ),
        ),
      ),
    );
  }
}

/// Toggle for auto-copy to clipboard setting.
class _AutoCopyToggle extends ConsumerWidget {
  @override
  Widget build(BuildContext context, WidgetRef ref) {
    final autoCopyEnabled = ref.watch(autoCopyEnabledProvider);

    return SwitchListTile(
      secondary: const Icon(Icons.content_copy),
      title: const Text('Auto-copy to clipboard'),
      subtitle: const Text('Copy transcript automatically when complete'),
      value: autoCopyEnabled,
      onChanged: (value) {
        ref.read(autoCopyEnabledProvider.notifier).toggle(value);
      },
    );
  }
}
