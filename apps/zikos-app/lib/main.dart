/// Production entrypoint: initialize the Rust bridge, hand it the app's
/// support directory, and run the UI over it. All rendering and routing
/// lives in `lib/src`; nothing here but boot order.
library;

import 'package:flutter/material.dart';
import 'package:path_provider/path_provider.dart';

import 'src/api/frb_zikos_api.dart';
import 'src/app.dart';
import 'src/rust/frb_generated.dart';

Future<void> main() async {
  WidgetsFlutterBinding.ensureInitialized();
  await RustLib.init();
  final api = const FrbZikosApi();
  final appDir = await getApplicationSupportDirectory();
  await api.init(appDir: appDir.path);
  runApp(ZikosApp(api: api));
}
