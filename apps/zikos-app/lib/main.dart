import 'package:flutter/material.dart';

void main() {
  runApp(const ZikosApp());
}

/// Zikos — end-to-end encrypted chat and a shared recording tree for
/// musicians.
///
/// UI shell only for now: conversations, the file tree browser and real
/// capture arrive as OpenSpec changes (see specs/platform-app.md). All
/// business logic lives in the Rust crates and reaches Dart through
/// flutter_rust_bridge; this file renders and routes only.
class ZikosApp extends StatelessWidget {
  const ZikosApp({super.key});

  @override
  Widget build(BuildContext context) {
    return MaterialApp(
      title: 'Zikos',
      theme: ThemeData(
        colorScheme: ColorScheme.fromSeed(seedColor: Colors.deepOrange),
      ),
      home: const ZikosHomePage(),
    );
  }
}

class ZikosHomePage extends StatefulWidget {
  const ZikosHomePage({super.key});

  @override
  State<ZikosHomePage> createState() => _ZikosHomePageState();
}

class _ZikosHomePageState extends State<ZikosHomePage> {
  bool _recording = false;

  void _toggleRecording() {
    setState(() {
      // Real capture goes through zikos-audio behind flutter_rust_bridge;
      // the always-one-tap-away state machine starts here.
      _recording = !_recording;
    });
  }

  @override
  Widget build(BuildContext context) {
    return Scaffold(
      appBar: AppBar(
        backgroundColor: Theme.of(context).colorScheme.inversePrimary,
        title: const Text('Zikos'),
      ),
      body: Center(
        child: Column(
          mainAxisAlignment: MainAxisAlignment.center,
          children: [
            Text(
              _recording ? 'Recording…' : 'Ready. One tap to record.',
              style: Theme.of(context).textTheme.titleMedium,
            ),
          ],
        ),
      ),
      floatingActionButton: FloatingActionButton.extended(
        onPressed: _toggleRecording,
        icon: Icon(_recording ? Icons.stop : Icons.mic),
        label: Text(_recording ? 'Stop' : 'Record'),
      ),
    );
  }
}
