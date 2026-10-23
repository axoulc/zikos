/// Record screen (specs/platform-app.md, "record screen"): one-tap start,
/// pause-aware timer, stop+finalize, interrupted-session recovery prompt,
/// capture surviving navigation.

library;

import 'package:flutter/material.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:provider/provider.dart';
import 'package:zikos_app/src/api/fake_zikos_api.dart';
import 'package:zikos_app/src/api/zikos_api.dart';
import 'package:zikos_app/src/app.dart';
import 'package:zikos_app/src/screens/record_screen.dart';

Future<void> pumpScreen(WidgetTester tester, FakeZikosApi api) async {
  await tester.pumpWidget(
    MaterialApp(
      home: Scaffold(
        body: Provider<ZikosApi>.value(
          value: api,
          child: const RecordScreen(),
        ),
      ),
    ),
  );
  await tester.pumpAndSettle();
}

void main() {
  testWidgets('idle: first tap starts a take and the timer runs',
      (tester) async {
    final api = FakeZikosApi();
    await pumpScreen(tester, api);

    expect(find.byKey(const ValueKey('record-start')), findsOneWidget);

    await tester.tap(find.byKey(const ValueKey('record-start')));
    await tester.pumpAndSettle();

    expect(api.recording.status, RecordingStatus.recording);
    expect(find.byKey(const ValueKey('recording-status')), findsOneWidget);

    // Elapsed time comes from the bridge stream; Dart only formats it.
    api.tick(42000);
    await tester.pumpAndSettle();
    expect(find.byKey(const ValueKey('record-timer')), findsOneWidget);
    expect(find.text('00:42'), findsOneWidget);
  });

  testWidgets('pause freezes the timer and toggles the control to resume',
      (tester) async {
    final api = FakeZikosApi();
    await pumpScreen(tester, api);
    await tester.tap(find.byKey(const ValueKey('record-start')));
    await tester.pumpAndSettle();
    api.tick(42000);
    await tester.pumpAndSettle();

    await tester.tap(find.byKey(const ValueKey('record-pause')));
    await tester.pumpAndSettle();

    expect(api.recording.status, RecordingStatus.paused);
    expect(find.text('Paused'), findsOneWidget);
    expect(find.text('00:42'), findsOneWidget, reason: 'timer is frozen');
    expect(find.byKey(const ValueKey('record-pause')), findsNothing);
    expect(find.byKey(const ValueKey('record-resume')), findsOneWidget);

    // Audio arriving while paused does not move the timer.
    api.tick(42000);
    await tester.pumpAndSettle();
    expect(find.text('00:42'), findsOneWidget);

    await tester.tap(find.byKey(const ValueKey('record-resume')));
    await tester.pumpAndSettle();
    expect(api.recording.status, RecordingStatus.recording);
  });

  testWidgets('stop finalizes the take and returns the screen to idle',
      (tester) async {
    final api = FakeZikosApi();
    await pumpScreen(tester, api);
    await tester.tap(find.byKey(const ValueKey('record-start')));
    await tester.pumpAndSettle();
    api.tick(42000);
    await tester.pumpAndSettle();

    await tester.tap(find.byKey(const ValueKey('record-stop')));
    await tester.pumpAndSettle();

    expect(api.finalizedTakes, hasLength(1));
    expect(api.finalizedTakes.single.durationSecs, 42);
    expect(api.recording.status, RecordingStatus.idle);
    expect(find.byKey(const ValueKey('record-start')), findsOneWidget);
  });

  testWidgets('an interrupted session prompts finalize-or-discard first',
      (tester) async {
    final api = FakeZikosApi()
      ..interrupted = InterruptedSessionView(startedAt: BigInt.from(1792656900));
    await pumpScreen(tester, api);

    expect(find.byKey(const ValueKey('interrupted-dialog')), findsOneWidget);

    // Starting a new take while the dialog stands is refused by the
    // bridge; the screen stays behind the dialog.
    await tester.tap(find.text('Discard'));
    await tester.pumpAndSettle();

    expect(find.byKey(const ValueKey('interrupted-dialog')), findsNothing);
    expect(api.finalizedTakes, isEmpty);
    expect(find.byKey(const ValueKey('record-start')), findsOneWidget);

    // Now a new take can start.
    await tester.tap(find.byKey(const ValueKey('record-start')));
    await tester.pumpAndSettle();
    expect(api.recording.status, RecordingStatus.recording);
  });

  testWidgets('finalizing the interrupted session keeps the take',
      (tester) async {
    final api = FakeZikosApi()
      ..interrupted = InterruptedSessionView(startedAt: BigInt.from(1792656900));
    await pumpScreen(tester, api);

    await tester.tap(find.text('Finalize'));
    await tester.pumpAndSettle();

    expect(api.finalizedTakes, hasLength(1));
    expect(find.byKey(const ValueKey('interrupted-dialog')), findsNothing);
    expect(find.byKey(const ValueKey('record-start')), findsOneWidget);
  });

  testWidgets('capture survives switching away and back (through the shell)',
      (tester) async {
    final api = FakeZikosApi();
    await tester.pumpWidget(ZikosApp(api: api));
    await tester.pumpAndSettle();

    await tester.tap(find.text('Record'));
    await tester.pumpAndSettle();
    await tester.tap(find.byKey(const ValueKey('record-start')));
    await tester.pumpAndSettle();
    api.tick(8000);
    await tester.pumpAndSettle();

    await tester.tap(find.text('Conversations'));
    await tester.pumpAndSettle();
    // The recording indicator is lit while on another tab.
    expect(
      find.byKey(const ValueKey('record-active-indicator')),
      findsOneWidget,
    );

    await tester.tap(find.text('Record'));
    await tester.pumpAndSettle();
    // Still recording with its elapsed time — the session is owned by the
    // Rust state, not the screen.
    expect(api.recording.status, RecordingStatus.recording);
    expect(find.text('00:08'), findsOneWidget);
  });
}
