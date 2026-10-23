/// Settings: homeserver, blob server URL, default recording format —
/// persisted through the bridge, invalid values rejected with a visible
/// error and never saved (specs/platform-app.md, "settings screen").
library;

import 'package:flutter/material.dart';
import 'package:provider/provider.dart';

import '../api/zikos_api.dart';

/// The Settings page, pushed from the shell's gear.
class SettingsScreen extends StatefulWidget {
  /// Creates the screen.
  const SettingsScreen({super.key});

  /// Route name the shell pushes.
  static const String route = '/settings';

  @override
  State<SettingsScreen> createState() => _SettingsScreenState();
}

class _SettingsScreenState extends State<SettingsScreen> {
  final _homeserver = TextEditingController();
  final _blobUrl = TextEditingController();
  String _format = 'flac';
  bool _loaded = false;
  String? _error;

  @override
  void initState() {
    super.initState();
    _load();
  }

  Future<void> _load() async {
    final settings = await context.read<ZikosApi>().settingsGet();
    if (!mounted) return;
    setState(() {
      _homeserver.text = settings.homeserverUrl;
      _blobUrl.text = settings.blobServerUrl;
      _format = settings.defaultFormat;
      _loaded = true;
    });
  }

  @override
  void dispose() {
    _homeserver.dispose();
    _blobUrl.dispose();
    super.dispose();
  }

  Future<void> _save() async {
    try {
      final settings = await context.read<ZikosApi>().settingsSave(
            homeserverUrl: _homeserver.text.trim(),
            blobServerUrl: _blobUrl.text.trim(),
            defaultFormat: _format,
          );
      if (!mounted) return;
      setState(() => _error = null);
      ScaffoldMessenger.of(context).showSnackBar(
        SnackBar(content: Text('Saved — takes will finalize to '
            '${settings.defaultFormat.toUpperCase()}')),
      );
    } catch (e) {
      if (!mounted) return;
      // The bridge rejected the values; keep them on screen (unsaved) so
      // the user can fix them.
      setState(() => _error = e.toString());
    }
  }

  @override
  Widget build(BuildContext context) {
    final theme = Theme.of(context);
    return Scaffold(
      key: const ValueKey('settings-screen'),
      appBar: AppBar(title: const Text('Settings')),
      body: !_loaded
          ? const Center(child: CircularProgressIndicator())
          : SingleChildScrollView(
              padding: const EdgeInsets.all(16),
              child: Column(
                crossAxisAlignment: CrossAxisAlignment.stretch,
                children: [
                  Text('Connection', style: theme.textTheme.titleSmall),
                  const SizedBox(height: 8),
                  TextField(
                    key: const ValueKey('field-homeserver'),
                    controller: _homeserver,
                    decoration: const InputDecoration(
                      labelText: 'Homeserver',
                      helperText: 'Matrix server used to sign in and sync.',
                      border: OutlineInputBorder(),
                    ),
                  ),
                  const SizedBox(height: 12),
                  TextField(
                    key: const ValueKey('field-blob-url'),
                    controller: _blobUrl,
                    decoration: const InputDecoration(
                      labelText: 'Blob server URL',
                      helperText: 'Where encrypted recordings are stored.',
                      border: OutlineInputBorder(),
                    ),
                  ),
                  const SizedBox(height: 24),
                  Text('Recording', style: theme.textTheme.titleSmall),
                  const SizedBox(height: 8),
                  DropdownButtonFormField<String>(
                    key: const ValueKey('field-format'),
                    initialValue: _format,
                    decoration: const InputDecoration(
                      labelText: 'Default format',
                      border: OutlineInputBorder(),
                    ),
                    items: const [
                      DropdownMenuItem(
                        value: 'flac',
                        child: Text('FLAC (lossless)'),
                      ),
                      DropdownMenuItem(
                        value: 'wav',
                        child: Text('WAV (raw)'),
                      ),
                    ],
                    onChanged: (value) =>
                        setState(() => _format = value ?? 'flac'),
                  ),
                  const SizedBox(height: 6),
                  Text(
                    'Takes finalize losslessly to FLAC by default; WAV keeps '
                    'the raw capture. New recordings use this format.',
                    style: theme.textTheme.bodySmall?.copyWith(
                      color: theme.colorScheme.onSurfaceVariant,
                    ),
                  ),
                  if (_error != null) ...[
                    const SizedBox(height: 16),
                    Container(
                      key: const ValueKey('settings-error'),
                      padding: const EdgeInsets.all(12),
                      decoration: BoxDecoration(
                        color: theme.colorScheme.errorContainer,
                        borderRadius: BorderRadius.circular(8),
                      ),
                      child: Row(
                        children: [
                          Icon(
                            Icons.error_outline,
                            color: theme.colorScheme.onErrorContainer,
                          ),
                          const SizedBox(width: 10),
                          Expanded(
                            child: Text(
                              _error!,
                              style: TextStyle(
                                color: theme.colorScheme.onErrorContainer,
                              ),
                            ),
                          ),
                        ],
                      ),
                    ),
                  ],
                  const SizedBox(height: 24),
                  FilledButton(
                    key: const ValueKey('save-settings'),
                    onPressed: _save,
                    child: const Text('Save'),
                  ),
                ],
              ),
            ),
    );
  }
}
