/// Files screen (specs/platform-app.md, "tree browser screen"): sorted
/// folder listing with duration/format chips and pending markers,
/// breadcrumb navigation, pick-a-conversation prompt, details sheet on
/// the third tap.

library;

import 'package:flutter/material.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:provider/provider.dart';
import 'package:zikos_app/src/api/fake_zikos_api.dart';
import 'package:zikos_app/src/api/zikos_api.dart';
import 'package:zikos_app/src/screens/files_screen.dart';

FakeZikosApi apiWithConversation({Map<String?, List<TreeNodeView>>? listings}) {
  final api = FakeZikosApi(
    signedIn: true,
    conversations: [fakeConversation('!jazz:example.org', 'Jazz Quartet')],
  )..selectConversation(roomId: '!jazz:example.org');
  listings?.forEach(api.setListing);
  return api;
}

Future<void> pumpScreen(WidgetTester tester, FakeZikosApi api) async {
  await tester.pumpWidget(
    MaterialApp(
      home: Scaffold(
        body: Provider<ZikosApi>.value(
          value: api,
          child: const FilesScreen(),
        ),
      ),
    ),
  );
  await tester.pumpAndSettle();
}

void main() {
  testWidgets('folder listing: folders first name-sorted, files with '
      'duration, format chip and pending marker', (tester) async {
    final api = apiWithConversation(listings: {
      null: [
        FakeZikosApi.folder('o', 'Oct 23', ['Oct 23']),
        FakeZikosApi.folder('r', 'Rehearsals', ['Rehearsals']),
        FakeZikosApi.file(
          'j',
          'jam-loop.wav',
          ['jam-loop.wav'],
          durationSecs: 48,
          format: 'wav',
        ),
        FakeZikosApi.file(
          't',
          'take-20261022-1930.flac',
          ['take-20261022-1930.flac'],
          durationSecs: 161,
          format: 'flac',
          pending: true,
        ),
      ],
    });
    await pumpScreen(tester, api);

    // The bridge returns rows already ordered (folders first, each group
    // name-sorted); the screen must render them in exactly that order.
    final names = tester
        .widgetList<ListTile>(find.byType(ListTile))
        .map((tile) => tile.title is Text ? (tile.title as Text).data : '')
        .toList();
    expect(names, [
      'Oct 23',
      'Rehearsals',
      'jam-loop.wav',
      'take-20261022-1930.flac',
    ]);

    expect(find.text('00:48'), findsOneWidget);
    expect(find.text('02:41'), findsOneWidget);
    expect(find.text('FLAC'), findsOneWidget);
    expect(find.text('WAV'), findsOneWidget);
    expect(find.byKey(const ValueKey('pending-marker')), findsOneWidget);
  });

  testWidgets('tapping a folder opens it; breadcrumb returns to ancestors',
      (tester) async {
    final api = apiWithConversation(listings: {
      null: [FakeZikosApi.folder('oct23', 'Oct 23', ['Oct 23'])],
      'oct23': [
        FakeZikosApi.folder('takes', 'Takes', ['Oct 23', 'Takes']),
        FakeZikosApi.file(
          'take1',
          'take-1.flac',
          ['Oct 23', 'take-1.flac'],
          durationSecs: 10,
          format: 'flac',
        ),
      ],
    });
    await pumpScreen(tester, api);

    await tester.tap(find.text('Oct 23'));
    await tester.pumpAndSettle();

    expect(find.text('Takes'), findsOneWidget);
    expect(find.text('take-1.flac'), findsOneWidget);
    // The breadcrumb names the current folder.
    expect(find.byKey(const ValueKey('crumb-Oct 23')), findsOneWidget);
    expect(find.byKey(const ValueKey('crumb-Tree')), findsOneWidget);

    // Tapping the root crumb returns to the root listing.
    await tester.tap(find.byKey(const ValueKey('crumb-Tree')));
    await tester.pumpAndSettle();
    expect(find.text('Oct 23'), findsOneWidget);
    expect(find.text('Takes'), findsNothing);
  });

  testWidgets('no conversation selected: prompt instead of an empty tree',
      (tester) async {
    final api = FakeZikosApi(signedIn: true);
    await pumpScreen(tester, api);

    expect(find.byKey(const ValueKey('no-conversation-state')), findsOneWidget);
    expect(find.byType(ListTile), findsNothing);
  });

  testWidgets('tapping a recording opens its details sheet (three taps '
      'from the main screen)', (tester) async {
    final api = apiWithConversation(listings: {
      null: [
        FakeZikosApi.file(
          'take1',
          'take-20261022-1930.flac',
          ['Oct 23', 'take-20261022-1930.flac'],
          durationSecs: 161,
          format: 'flac',
        ),
      ],
    });
    await pumpScreen(tester, api);

    await tester.tap(find.text('take-20261022-1930.flac'));
    await tester.pumpAndSettle();

    final sheet = find.byKey(const ValueKey('recording-details-sheet'));
    expect(sheet, findsOneWidget);
    expect(
      find.descendant(of: sheet, matching: find.text('take-20261022-1930.flac')),
      findsOneWidget,
    );
    expect(find.descendant(of: sheet, matching: find.text('02:41')),
        findsOneWidget);
    expect(find.descendant(of: sheet, matching: find.text('FLAC')),
        findsOneWidget);
    // The tree path is shown with the conversation context.
    expect(find.textContaining('Oct 23'), findsWidgets);
  });
}
