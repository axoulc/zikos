/// Settings screen (specs/platform-app.md, "settings screen"): homeserver,
/// blob server URL, default recording format — persisted via the bridge,
/// invalid values rejected visibly.

library;

import 'package:flutter/material.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:provider/provider.dart';
import 'package:zikos_app/src/api/fake_zikos_api.dart';
import 'package:zikos_app/src/api/zikos_api.dart';
import 'package:zikos_app/src/screens/settings_screen.dart';

Future<void> pumpScreen(WidgetTester tester, FakeZikosApi api) async {
  await tester.pumpWidget(
    MaterialApp(
      home: Provider<ZikosApi>.value(
        value: api,
        child: const SettingsScreen(),
      ),
    ),
  );
  await tester.pumpAndSettle();
}

void main() {
  testWidgets('fields show current persisted values', (tester) async {
    final api = FakeZikosApi(
      settings: const SettingsView(
        homeserverUrl: 'https://matrix.example.org',
        blobServerUrl: 'https://blob.example.org',
        defaultFormat: 'wav',
      ),
    );
    await pumpScreen(tester, api);

    expect(find.text('https://matrix.example.org'), findsOneWidget);
    expect(find.text('https://blob.example.org'), findsOneWidget);
    expect(find.text('WAV (raw)'), findsOneWidget);
  });

  testWidgets('a valid save persists through the api', (tester) async {
    final api = FakeZikosApi();
    await pumpScreen(tester, api);

    await tester.enterText(
      find.byKey(const ValueKey('field-homeserver')),
      'https://matrix.example.org',
    );
    await tester.enterText(
      find.byKey(const ValueKey('field-blob-url')),
      'https://blob.example.org',
    );
    await tester.tap(find.byKey(const ValueKey('save-settings')));
    await tester.pumpAndSettle();

    expect(api.settings.homeserverUrl, 'https://matrix.example.org');
    expect(api.settings.blobServerUrl, 'https://blob.example.org');
    expect(find.byKey(const ValueKey('settings-error')), findsNothing);
  });

  testWidgets('a malformed URL is rejected visibly and keeps the old value',
      (tester) async {
    final api = FakeZikosApi();
    await pumpScreen(tester, api);

    await tester.enterText(
      find.byKey(const ValueKey('field-homeserver')),
      'htp://matrix.example.org',
    );
    await tester.tap(find.byKey(const ValueKey('save-settings')));
    await tester.pumpAndSettle();

    expect(find.byKey(const ValueKey('settings-error')), findsOneWidget);
    expect(find.textContaining('https:// URL'), findsOneWidget);
    // Nothing was persisted.
    expect(api.settings.homeserverUrl, '');
  });

  testWidgets('the format defaults to FLAC and WAV is selectable; Opus is '
      'not offered', (tester) async {
    final api = FakeZikosApi();
    await pumpScreen(tester, api);

    expect(find.text('FLAC (lossless)'), findsOneWidget);

    await tester.tap(find.byKey(const ValueKey('field-format')));
    await tester.pumpAndSettle();
    expect(find.textContaining('Opus'), findsNothing);
    await tester.tap(find.text('WAV (raw)'));
    await tester.pumpAndSettle();

    await tester.tap(find.byKey(const ValueKey('save-settings')));
    await tester.pumpAndSettle();

    expect(api.settings.defaultFormat, 'wav');
  });
}
