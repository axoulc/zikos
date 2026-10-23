/// Record: the dictaphone — one-tap start, pause-aware tabular timer,
/// stop+finalize, interrupted-session recovery prompt
/// (specs/platform-app.md, "record screen"; specs/audio-recording.md,
/// "one-tap recording").
library;

import 'dart:async';

import 'package:flutter/material.dart';
import 'package:provider/provider.dart';

import '../api/zikos_api.dart';

/// The Record destination.
class RecordScreen extends StatefulWidget {
  /// Creates the screen.
  const RecordScreen({super.key});

  @override
  State<RecordScreen> createState() => _RecordScreenState();
}

class _RecordScreenState extends State<RecordScreen> {
  RecordingStateView _state = const RecordingStateView(
    status: RecordingStatus.idle,
    elapsedMs: 0,
  );
  ConversationView? _conversation;
  String _defaultFormat = 'FLAC';
  StreamSubscription<RecordingStateView>? _stateSub;
  bool _interruptedPromptShown = false;

  @override
  void initState() {
    super.initState();
    final api = context.read<ZikosApi>();
    _stateSub = api.recordingStateStream().listen((state) {
      if (!mounted) return;
      setState(() => _state = state);
    });
    _loadContext(api);
    _offerRecovery(api);
  }

  Future<void> _loadContext(ZikosApi api) async {
    final conversation = await api.currentConversation();
    final settings = await api.settingsGet();
    if (!mounted) return;
    setState(() {
      _conversation = conversation;
      _defaultFormat = settings.defaultFormat.toUpperCase();
    });
  }

  Future<void> _offerRecovery(ZikosApi api) async {
    final interrupted = await api.interruptedSession();
    if (!mounted || interrupted == null || _interruptedPromptShown) return;
    _interruptedPromptShown = true;
    await showDialog<void>(
      context: context,
      barrierDismissible: false,
      builder: (dialogContext) => _InterruptedDialog(api: api),
    );
    if (!mounted) return;
    // The decision may have changed what a new take would do; re-check.
    setState(() {});
  }

  @override
  void dispose() {
    _stateSub?.cancel();
    super.dispose();
  }

  Future<void> _command(Future<void> Function(ZikosApi api) action) async {
    final api = context.read<ZikosApi>();
    try {
      await action(api);
    } catch (e) {
      if (!mounted) return;
      ScaffoldMessenger.of(context).showSnackBar(
        SnackBar(content: Text(e.toString())),
      );
    }
  }

  @override
  Widget build(BuildContext context) {
    final theme = Theme.of(context);
    final recording = _state.status != RecordingStatus.idle;
    return KeyedSubtree(
      key: const ValueKey('record-screen'),
      child: Center(
        child: Column(
          mainAxisAlignment: MainAxisAlignment.center,
          children: [
            if (_conversation != null)
              Padding(
                padding: const EdgeInsets.only(bottom: 20),
                child: Chip(
                  avatar: const Icon(Icons.lock, size: 14),
                  label: Text(_conversation!.displayName),
                  visualDensity: VisualDensity.compact,
                ),
              ),
            if (recording) ...[
              Row(
                mainAxisAlignment: MainAxisAlignment.center,
                children: [
                  Icon(
                    Icons.circle,
                    size: 12,
                    color: _state.status == RecordingStatus.recording
                        ? theme.colorScheme.error
                        : theme.colorScheme.outline,
                  ),
                  const SizedBox(width: 8),
                  Text(
                    _state.status == RecordingStatus.recording
                        ? 'Recording'
                        : 'Paused',
                    key: const ValueKey('recording-status'),
                    style: theme.textTheme.labelLarge?.copyWith(
                      color: _state.status == RecordingStatus.recording
                          ? theme.colorScheme.error
                          : theme.colorScheme.onSurfaceVariant,
                      letterSpacing: 1.2,
                    ),
                  ),
                ],
              ),
              const SizedBox(height: 10),
              Text(
                _clock(_state.elapsedMs),
                key: const ValueKey('record-timer'),
                style: theme.textTheme.displayMedium?.copyWith(
                  fontWeight: FontWeight.w500,
                  fontFeatures: const [FontFeature.tabularFigures()],
                ),
              ),
            ] else
              Padding(
                padding: const EdgeInsets.only(bottom: 24),
                child: Text(
                  'Ready. One tap to record.',
                  style: theme.textTheme.titleMedium,
                ),
              ),
            const SizedBox(height: 28),
            Row(
              mainAxisAlignment: MainAxisAlignment.center,
              children: [
                if (recording) ...[
                  Column(
                    children: [
                      FloatingActionButton.small(
                        key: ValueKey(
                          _state.status == RecordingStatus.paused
                              ? 'record-resume'
                              : 'record-pause',
                        ),
                        onPressed: () => _command(
                          (api) => _state.status == RecordingStatus.paused
                              ? api.recordResume()
                              : api.recordPause(),
                        ),
                        child: Icon(
                          _state.status == RecordingStatus.paused
                              ? Icons.play_arrow
                              : Icons.pause,
                        ),
                      ),
                      const SizedBox(height: 6),
                      Text(
                        _state.status == RecordingStatus.paused
                            ? 'Resume'
                            : 'Pause',
                        style: theme.textTheme.labelSmall,
                      ),
                    ],
                  ),
                  const SizedBox(width: 28),
                ],
                Column(
                  children: [
                    recording
                        ? FloatingActionButton.large(
                            key: const ValueKey('record-stop'),
                            backgroundColor: theme.colorScheme.primary,
                            foregroundColor: theme.colorScheme.onPrimary,
                            onPressed: () => _command((api) => api.recordStop()),
                            child: const Icon(Icons.stop),
                          )
                        : FloatingActionButton.large(
                            key: const ValueKey('record-start'),
                            backgroundColor: theme.colorScheme.primary,
                            foregroundColor: theme.colorScheme.onPrimary,
                            onPressed: () => _command((api) => api.recordStart()),
                            child: const Icon(Icons.mic),
                          ),
                    const SizedBox(height: 6),
                    Text(
                      recording ? 'Stop' : 'Record',
                      style: theme.textTheme.labelSmall,
                    ),
                  ],
                ),
              ],
            ),
            const SizedBox(height: 28),
            Text(
              recording
                  ? 'Stop finalizes this take to $_defaultFormat and files it '
                      'in ${_conversation?.displayName ?? 'your conversation'}'
                  : 'Takes finalize losslessly to $_defaultFormat by default.',
              textAlign: TextAlign.center,
              style: theme.textTheme.bodySmall?.copyWith(
                color: theme.colorScheme.onSurfaceVariant,
              ),
            ),
          ],
        ),
      ),
    );
  }

  String _clock(int elapsedMs) {
    final total = elapsedMs ~/ 1000;
    final m = (total ~/ 60).toString().padLeft(2, '0');
    final s = (total % 60).toString().padLeft(2, '0');
    return '$m:$s';
  }
}

class _InterruptedDialog extends StatelessWidget {
  const _InterruptedDialog({required this.api});

  final ZikosApi api;

  @override
  Widget build(BuildContext context) {
    return AlertDialog(
      key: const ValueKey('interrupted-dialog'),
      title: const Text('Interrupted take found'),
      content: const Text(
        'A previous recording was interrupted before it finished. Finalize '
        'it as a take, or discard it. A new take cannot start until you '
        'decide.',
      ),
      actions: [
        TextButton(
          onPressed: () async {
            try {
              await api.interruptedDiscard();
            } finally {
              if (context.mounted) Navigator.of(context).pop();
            }
          },
          child: const Text('Discard'),
        ),
        FilledButton(
          onPressed: () async {
            try {
              await api.interruptedFinalize();
            } finally {
              if (context.mounted) Navigator.of(context).pop();
            }
          },
          child: const Text('Finalize'),
        ),
      ],
    );
  }
}
