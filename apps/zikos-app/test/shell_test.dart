/// Adaptive navigation shell (specs/platform-app.md, "main navigation"):
/// three destinations always visible, Record one tap away, bottom bar on
/// narrow screens and a rail on desktop, gear → Settings.

library;

import 'package:flutter/material.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:zikos_app/src/api/fake_zikos_api.dart';
import 'package:zikos_app/src/api/zikos_api.dart';
import 'package:zikos_app/src/app.dart';
import 'package:provider/provider.dart';

Widget appWith(FakeZikosApi api) => ZikosApp(api: api);

Future<void> pumpNarrow(WidgetTester tester, FakeZikosApi api) async {
  tester.view.physicalSize = const Size(400, 800);
  tester.view.devicePixelRatio = 1.0;
  addTearDown(tester.view.resetPhysicalSize);
  addTearDown(tester.view.resetDevicePixelRatio);
  await tester.pumpWidget(appWith(api));
  await tester.pumpAndSettle();
}

Future<void> pumpWide(WidgetTester tester, FakeZikosApi api) async {
  tester.view.physicalSize = const Size(1280, 800);
  tester.view.devicePixelRatio = 1.0;
  addTearDown(tester.view.resetPhysicalSize);
  addTearDown(tester.view.resetDevicePixelRatio);
  await tester.pumpWidget(appWith(api));
  await tester.pumpAndSettle();
}

void main() {
  testWidgets('narrow screens use a bottom bar with three destinations',
      (tester) async {
    final api = FakeZikosApi();
    await pumpNarrow(tester, api);

    expect(find.byType(NavigationBar), findsOneWidget);
    expect(find.byType(NavigationRail), findsNothing);
    expect(find.byKey(const ValueKey('conversations-screen')), findsOneWidget);

    // Record is one tap away from Conversations.
    await tester.tap(find.text('Record'));
    await tester.pumpAndSettle();
    expect(find.byKey(const ValueKey('record-screen')), findsOneWidget);

    await tester.tap(find.text('Files'));
    await tester.pumpAndSettle();
    expect(find.byKey(const ValueKey('files-screen')), findsOneWidget);

    await tester.tap(find.text('Conversations'));
    await tester.pumpAndSettle();
    expect(find.byKey(const ValueKey('conversations-screen')), findsOneWidget);
  });

  testWidgets('wide screens use a navigation rail, no bottom bar',
      (tester) async {
    final api = FakeZikosApi();
    await pumpWide(tester, api);

    expect(find.byType(NavigationRail), findsOneWidget);
    expect(find.byType(NavigationBar), findsNothing);

    await tester.tap(find.text('Record'));
    await tester.pumpAndSettle();
    expect(find.byKey(const ValueKey('record-screen')), findsOneWidget);
  });

  testWidgets('the app-bar gear opens settings', (tester) async {
    final api = FakeZikosApi();
    await pumpNarrow(tester, api);

    await tester.tap(find.byKey(const ValueKey('open-settings')));
    await tester.pumpAndSettle();

    expect(find.byKey(const ValueKey('settings-screen')), findsOneWidget);
    expect(find.byType(BackButton), findsOneWidget);
  });

  testWidgets('an active recording lights the indicator on the Record '
      'destination from any tab', (tester) async {
    final api = FakeZikosApi();
    await pumpNarrow(tester, api);

    // Idle: no indicator anywhere.
    expect(find.byKey(const ValueKey('record-active-indicator')), findsNothing);

    api.recordingController.add(const RecordingStateView(
      status: RecordingStatus.recording,
      elapsedMs: 1000,
    ));
    await tester.pumpAndSettle();

    // From the Conversations tab the Record destination shows it.
    expect(find.byKey(const ValueKey('record-active-indicator')), findsOneWidget);

    // It stays lit on the Files tab too.
    await tester.tap(find.text('Files'));
    await tester.pumpAndSettle();
    expect(find.byKey(const ValueKey('record-active-indicator')), findsOneWidget);

    // And goes out when the take stops.
    api.recordingController.add(const RecordingStateView(
      status: RecordingStatus.idle,
      elapsedMs: 0,
    ));
    await tester.pumpAndSettle();
    expect(find.byKey(const ValueKey('record-active-indicator')), findsNothing);
  });

  testWidgets('the shell provides the api to descendants', (tester) async {
    final api = FakeZikosApi();
    await pumpNarrow(tester, api);
    // Sanity: some widget below consumed the provider without exploding.
    expect(find.byType(Provider<ZikosApi>), findsOneWidget);
  });
}
