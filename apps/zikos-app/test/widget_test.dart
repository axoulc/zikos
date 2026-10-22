import 'package:flutter_test/flutter_test.dart';

import 'package:zikos_app/main.dart';

void main() {
  testWidgets('record button toggles recording state', (WidgetTester tester) async {
    await tester.pumpWidget(const ZikosApp());

    expect(find.text('Zikos'), findsOneWidget);
    expect(find.text('Ready. One tap to record.'), findsOneWidget);
    expect(find.text('Record'), findsOneWidget);

    await tester.tap(find.text('Record'));
    await tester.pump();

    expect(find.text('Recording…'), findsOneWidget);
    expect(find.text('Stop'), findsOneWidget);

    await tester.tap(find.text('Stop'));
    await tester.pump();

    expect(find.text('Ready. One tap to record.'), findsOneWidget);
    expect(find.text('Record'), findsOneWidget);
  });
}
