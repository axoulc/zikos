/// Adaptive main navigation: Conversations, Files, Record — bottom bar
/// on narrow screens, rail on desktop (specs/platform-app.md).
library;

import 'dart:async';

import 'package:flutter/material.dart';
import 'package:provider/provider.dart';

import 'api/zikos_api.dart';
import 'screens/conversations_screen.dart';
import 'screens/files_screen.dart';
import 'screens/record_screen.dart';
import 'screens/settings_screen.dart';

/// Width at which the shell switches to the desktop rail.
const kDesktopBreakpoint = 600.0;

/// The main screen with the three destinations and the settings entry.
class ZikosShell extends StatefulWidget {
  /// Creates the shell.
  const ZikosShell({super.key});

  @override
  State<ZikosShell> createState() => _ZikosShellState();
}

class _ZikosShellState extends State<ZikosShell> {
  int _selected = 0;
  bool _recordingActive = false;
  StreamSubscription<RecordingStateView>? _recordingSub;

  static const _destinations = [
    (Icons.forum_outlined, Icons.forum, 'Conversations'),
    (Icons.folder_outlined, Icons.folder, 'Files'),
    (Icons.mic_none, Icons.mic, 'Record'),
  ];

  static const _screens = [
    ConversationsScreen(),
    FilesScreen(),
    RecordScreen(),
  ];

  @override
  void initState() {
    super.initState();
    final api = context.read<ZikosApi>();
    _recordingSub = api.recordingStateStream().listen((state) {
      if (!mounted) return;
      setState(() => _recordingActive = state.isActive);
    });
  }

  @override
  void dispose() {
    _recordingSub?.cancel();
    super.dispose();
  }

  Widget _destinationIcon(IconData outline, IconData filled, int index) {
    final icon = Icon(_selected == index ? filled : outline);
    if (index != 2 || !_recordingActive) return icon;
    // The Record destination carries the active-recording indicator so
    // capture is visibly alive from any tab; the badge exists only while
    // a take is active.
    return Badge(
      key: const ValueKey('record-active-indicator'),
      smallSize: 10,
      backgroundColor: Theme.of(context).colorScheme.error,
      child: icon,
    );
  }

  @override
  Widget build(BuildContext context) {
    final wide = MediaQuery.sizeOf(context).width >= kDesktopBreakpoint;
    final body = IndexedStack(index: _selected, children: _screens);

    if (wide) {
      return Scaffold(
        body: Row(
          children: [
            NavigationRail(
              selectedIndex: _selected,
              onDestinationSelected: _select,
              labelType: NavigationRailLabelType.all,
              leading: Padding(
                padding: const EdgeInsets.only(bottom: 12),
                child: _settingsButton(),
              ),
              destinations: [
                for (final (index, (_, _, label)) in _destinations.indexed)
                  NavigationRailDestination(
                    icon: _destinationIcon(
                      _destinations[index].$1,
                      _destinations[index].$2,
                      index,
                    ),
                    label: Text(label),
                  ),
              ],
            ),
            const VerticalDivider(width: 1),
            Expanded(child: body),
          ],
        ),
      );
    }

    return Scaffold(
      appBar: AppBar(
        backgroundColor: Theme.of(context).colorScheme.inversePrimary,
        title: const Text('Zikos'),
        actions: [_settingsButton()],
      ),
      body: body,
      bottomNavigationBar: NavigationBar(
        selectedIndex: _selected,
        onDestinationSelected: _select,
        destinations: [
          for (final (index, (_, _, label)) in _destinations.indexed)
            NavigationDestination(
              icon: _destinationIcon(
                _destinations[index].$1,
                _destinations[index].$2,
                index,
              ),
              label: label,
            ),
        ],
      ),
    );
  }

  void _select(int index) => setState(() => _selected = index);

  Widget _settingsButton() => IconButton(
        key: const ValueKey('open-settings'),
        icon: const Icon(Icons.settings_outlined),
        tooltip: 'Settings',
        onPressed: () => Navigator.of(context).pushNamed(SettingsScreen.route),
      );
}
