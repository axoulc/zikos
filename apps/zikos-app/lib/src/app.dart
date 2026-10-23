/// App root: theme, api provider, shell, and the settings route.
library;

import 'package:flutter/material.dart';
import 'package:provider/provider.dart';

import 'api/zikos_api.dart';
import 'screens/settings_screen.dart';
import 'shell.dart';

/// The Zikos application over an injected [ZikosApi].
class ZikosApp extends StatelessWidget {
  /// Creates the app; tests inject a fake, main the FRB-backed one.
  const ZikosApp({super.key, required this.api});

  /// The bridge to the Rust core.
  final ZikosApi api;

  @override
  Widget build(BuildContext context) {
    return Provider<ZikosApi>.value(
      value: api,
      child: MaterialApp(
        title: 'Zikos',
        theme: ThemeData(
          colorScheme: ColorScheme.fromSeed(seedColor: Colors.deepOrange),
        ),
        home: const ZikosShell(),
        routes: {
          SettingsScreen.route: (_) => const SettingsScreen(),
        },
      ),
    );
  }
}
