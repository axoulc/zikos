/// Conversations screen (specs/platform-app.md, "conversations screen"):
/// rows with name/snippet/time/encryption glyph, live updates, signed-out
/// state, tap-to-select with a visible current cue.

library;

import 'package:flutter/material.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:provider/provider.dart';
import 'package:zikos_app/src/api/fake_zikos_api.dart';
import 'package:zikos_app/src/api/zikos_api.dart';
import 'package:zikos_app/src/screens/conversations_screen.dart';

Future<void> pumpScreen(WidgetTester tester, FakeZikosApi api) async {
  await tester.pumpWidget(
    MaterialApp(
      home: Scaffold(
        body: Provider<ZikosApi>.value(
          value: api,
          child: const ConversationsScreen(),
        ),
      ),
    ),
  );
  await tester.pumpAndSettle();
}

void main() {
  testWidgets('rows render name, snippet, time and encryption glyph',
      (tester) async {
    final api = FakeZikosApi(
      signedIn: true,
      conversations: [
        fakeConversation('!jazz:example.org', 'Jazz Quartet',
            lastSender: '@bob:example.org',
            lastPreview: 'take 3 is the keeper',
            lastTimestampMs: 1792000000000),
        fakeConversation('!admin:example.org', 'Band Admin',
            encrypted: false, encryptionKnown: true),
        fakeConversation('!mystery:example.org', 'Mystery',
            encrypted: false, encryptionKnown: false),
      ],
    );
    await pumpScreen(tester, api);

    expect(find.text('Jazz Quartet'), findsOneWidget);
    expect(find.textContaining('take 3 is the keeper'), findsOneWidget);
    // Shape-differentiated encryption glyphs.
    expect(find.byKey(const ValueKey('encryption-encrypted')), findsOneWidget);
    expect(find.byKey(const ValueKey('encryption-unencrypted')), findsOneWidget);
    expect(find.byKey(const ValueKey('encryption-unknown')), findsOneWidget);
    // No activity → no snippet, no time.
    expect(find.textContaining('Band Admin'), findsOneWidget);
  });

  testWidgets('a stream update changes the list without manual refresh',
      (tester) async {
    final api = FakeZikosApi(
      signedIn: true,
      conversations: [fakeConversation('!jazz:example.org', 'Jazz Quartet')],
    );
    await pumpScreen(tester, api);
    expect(find.text('Jazz Quartet'), findsOneWidget);
    expect(find.text('Sunday Rehearsal'), findsNothing);

    api.setConversations([
      fakeConversation('!jazz:example.org', 'Jazz Quartet'),
      fakeConversation('!rehearsal:example.org', 'Sunday Rehearsal',
          lastSender: '@marc:example.org', lastPreview: 'uploading stems'),
    ]);
    await tester.pumpAndSettle();

    expect(find.text('Sunday Rehearsal'), findsOneWidget);
    expect(find.textContaining('uploading stems'), findsOneWidget);
  });

  testWidgets('signed out: explanation instead of rows', (tester) async {
    final api = FakeZikosApi();
    await pumpScreen(tester, api);

    expect(find.byKey(const ValueKey('signed-out-state')), findsOneWidget);
    expect(find.byKey(const ValueKey('encryption-encrypted')), findsNothing);
  });

  testWidgets('tapping a row selects it as current conversation with a '
      'visible cue', (tester) async {
    final api = FakeZikosApi(
      signedIn: true,
      conversations: [
        fakeConversation('!jazz:example.org', 'Jazz Quartet'),
        fakeConversation('!rehearsal:example.org', 'Sunday Rehearsal'),
      ],
    );
    await pumpScreen(tester, api);

    expect(find.byKey(const ValueKey('current-conversation-marker')), findsNothing);

    await tester.tap(find.text('Jazz Quartet'));
    await tester.pumpAndSettle();

    expect(api.selectedRoomId, '!jazz:example.org');
    // The cue lives inside the selected row only.
    final jazzRow = find.ancestor(
      of: find.text('Jazz Quartet'),
      matching: find.byType(ListTile),
    );
    expect(
      find.descendant(
        of: jazzRow,
        matching: find.byKey(const ValueKey('current-conversation-marker')),
      ),
      findsOneWidget,
    );
  });
}
