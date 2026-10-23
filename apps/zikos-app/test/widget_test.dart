/// Smoke: the real app root pumps with the fake api injected — screens,
/// navigation and settings route all come up without any Rust dylib
/// (production boot lives in `lib/main.dart` behind RustLib.init()).

library;

import 'package:flutter/material.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:zikos_app/src/api/fake_zikos_api.dart';
import 'package:zikos_app/src/app.dart';

void main() {
  testWidgets('the app boots to the conversations destination',
      (tester) async {
    tester.view.physicalSize = const Size(400, 800);
    tester.view.devicePixelRatio = 1.0;
    addTearDown(tester.view.resetPhysicalSize);
    addTearDown(tester.view.resetDevicePixelRatio);

    await tester.pumpWidget(ZikosApp(api: FakeZikosApi()));
    await tester.pumpAndSettle();

    expect(find.text('Zikos'), findsOneWidget);
    expect(find.byKey(const ValueKey('conversations-screen')), findsOneWidget);
    expect(find.byKey(const ValueKey('signed-out-state')), findsOneWidget);
    expect(find.byType(NavigationBar), findsOneWidget);

    // All three destinations switch.
    await tester.tap(find.text('Files'));
    await tester.pumpAndSettle();
    expect(find.byKey(const ValueKey('files-screen')), findsOneWidget);

    await tester.tap(find.text('Record'));
    await tester.pumpAndSettle();
    expect(find.byKey(const ValueKey('record-screen')), findsOneWidget);
    expect(find.byKey(const ValueKey('record-start')), findsOneWidget);
  });
}
