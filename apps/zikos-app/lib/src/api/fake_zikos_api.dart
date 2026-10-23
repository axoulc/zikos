/// In-memory [ZikosApi] for widget tests: screens render and routes
/// against this without any Rust dylib. Behavior mirrors the Rust side's
/// documented semantics (one active take, paused spans excluded, a
/// pending interrupted session blocks new takes).
library;

import 'dart:async';

import 'zikos_api.dart';

/// Scriptable fake backing every widget test.
class FakeZikosApi implements ZikosApi {
  /// Instantiated state, for assertions.
  FakeZikosApi({
    List<ConversationView>? conversations,
    SettingsView? settings,
    this.signedIn = false,
  }) : settings = settings ?? _defaultSettings() {
    if (conversations != null) {
      _conversations.addAll(conversations);
    }
  }

  static SettingsView _defaultSettings() => const SettingsView(
        homeserverUrl: '',
        blobServerUrl: '',
        defaultFormat: 'flac',
      );

  /// Whether a Matrix session exists.
  bool signedIn;

  /// Persisted settings (mutated by [settingsSave]).
  SettingsView settings;

  final List<ConversationView> _conversations = [];
  final Map<String?, List<TreeNodeView>> _listings = {};
  String? _selectedRoomId;
  String? _selectedRoomIdGetOverride;

  RecordingStateView _recording = const RecordingStateView(
    status: RecordingStatus.idle,
    elapsedMs: 0,
  );
  InterruptedSessionView? _interrupted;

  /// Emits every recording-state change (test drives this directly).
  final StreamController<RecordingStateView> recordingController =
      StreamController<RecordingStateView>.broadcast();

  /// Emits conversation-list snapshots.
  final StreamController<List<ConversationView>> conversationsController =
      StreamController<List<ConversationView>>.broadcast();

  /// Emits session snapshots.
  final StreamController<SessionView> sessionController =
      StreamController<SessionView>.broadcast();

  /// Last error message handed to a throwing call, for assertions.
  String? lastError;

  /// Takes finalized via [recordStop], newest last.
  final List<TakeView> finalizedTakes = [];

  /// Whether [recordStart] should throw (e.g. interrupted pending).
  bool failRecordStart = false;

  /// Install a folder listing (`null` key = tree root).
  void setListing(String? folderId, List<TreeNodeView> rows) {
    _listings[folderId] = rows;
  }

  /// Push a conversation set and notify listeners of the refresh.
  void setConversations(List<ConversationView> conversations) {
    _conversations
      ..clear()
      ..addAll(conversations);
    conversationsController.add(List.of(_conversations));
  }

  /// Force what [currentConversation] returns (e.g. after a refresh
  /// that the fake would otherwise not mirror).
  set currentConversationOverride(ConversationView? view) {
    _selectedRoomIdGetOverride = view?.roomId;
    _selectedRoomId = view?.roomId ?? _selectedRoomId;
  }

  /// Last room id passed to [selectConversation], for assertions.
  String? get selectedRoomId => _selectedRoomId;

  /// Current recording state, for assertions.
  RecordingStateView get recording => _recording;

  /// Advance the fake recorder clock (keeping the current status) and
  /// notify subscribers — models what the Rust pump thread reports.
  void tick(int elapsedMs) {
    _recording = RecordingStateView(
      status: _recording.status,
      elapsedMs: elapsedMs,
    );
    recordingController.add(_recording);
  }

  /// Simulate an interrupted session waiting for a decision.
  set interrupted(InterruptedSessionView? view) => _interrupted = view;

  @override
  Future<AppSnapshot> init({required String appDir}) async => AppSnapshot(
        session: SessionView(signedIn: signedIn, userId: signedIn ? '@alice:example.org' : null),
        settings: settings,
      );

  @override
  Stream<SessionView> sessionStream() async* {
    yield SessionView(
      signedIn: signedIn,
      userId: signedIn ? '@alice:example.org' : null,
    );
    yield* sessionController.stream;
  }

  @override
  Future<SettingsView> settingsGet() async => settings;

  @override
  Future<SettingsView> settingsSave({
    required String homeserverUrl,
    required String blobServerUrl,
    required String defaultFormat,
  }) async {
    final candidate = SettingsView(
      homeserverUrl: homeserverUrl,
      blobServerUrl: blobServerUrl,
      defaultFormat: defaultFormat,
    );
    // Mirror of the Rust validation: https-only, flac/wav only.
    for (final url in [candidate.homeserverUrl, candidate.blobServerUrl]) {
      if (url.isNotEmpty && !url.startsWith('https://')) {
        lastError = 'must be a valid https:// URL';
        throw Exception(lastError);
      }
    }
    if (defaultFormat != 'flac' && defaultFormat != 'wav') {
      lastError = 'unsupported format';
      throw Exception(lastError);
    }
    settings = candidate;
    return settings;
  }

  @override
  Future<List<ConversationView>> conversations() async =>
      List.of(_conversations);

  @override
  Stream<List<ConversationView>> conversationsStream() async* {
    yield List.of(_conversations);
    yield* conversationsController.stream;
  }

  @override
  Future<List<ConversationView>> refreshConversations() async =>
      conversations();

  @override
  Future<void> selectConversation({required String roomId}) async {
    _selectedRoomId = roomId;
    _selectedRoomIdGetOverride = roomId;
  }

  @override
  Future<ConversationView?> currentConversation() async {
    final id = _selectedRoomIdGetOverride ?? _selectedRoomId;
    if (id == null) return null;
    for (final conversation in _conversations) {
      if (conversation.roomId == id) return conversation;
    }
    return null;
  }

  @override
  Future<List<TreeNodeView>> folderListing({String? folderId}) async {
    if (!signedIn) {
      lastError = 'not signed in';
      throw Exception(lastError);
    }
    if (await currentConversation() == null) {
      lastError = 'no conversation selected';
      throw Exception(lastError);
    }
    return List.of(_listings[folderId] ?? const []);
  }

  @override
  Future<RecordingStateView> recordingState() async => _recording;

  @override
  Stream<RecordingStateView> recordingStateStream() async* {
    yield _recording;
    yield* recordingController.stream;
  }

  @override
  Future<void> recordStart() async {
    if (failRecordStart || _interrupted != null) {
      lastError = 'an interrupted session is waiting';
      throw Exception(lastError);
    }
    if (_recording.status != RecordingStatus.idle) {
      lastError = 'a recording is already active';
      throw Exception(lastError);
    }
    _recording = const RecordingStateView(
      status: RecordingStatus.recording,
      elapsedMs: 0,
    );
    recordingController.add(_recording);
  }

  @override
  Future<void> recordPause() async {
    if (_recording.status != RecordingStatus.recording) {
      lastError = 'no recording is active';
      throw Exception(lastError);
    }
    _recording = RecordingStateView(
      status: RecordingStatus.paused,
      elapsedMs: _recording.elapsedMs,
    );
    recordingController.add(_recording);
  }

  @override
  Future<void> recordResume() async {
    if (_recording.status != RecordingStatus.paused) {
      lastError = 'not paused';
      throw Exception(lastError);
    }
    _recording = RecordingStateView(
      status: RecordingStatus.recording,
      elapsedMs: _recording.elapsedMs,
    );
    recordingController.add(_recording);
  }

  @override
  Future<TakeView> recordStop() async {
    if (_recording.status == RecordingStatus.idle) {
      lastError = 'no recording is active';
      throw Exception(lastError);
    }
    final take = TakeView(
      title: 'take-20261023-1930',
      format: settings.defaultFormat,
      durationSecs: _recording.elapsedMs / 1000,
      audioPath: '/tmp/sessions/take.${settings.defaultFormat}',
    );
    finalizedTakes.add(take);
    _recording = const RecordingStateView(
      status: RecordingStatus.idle,
      elapsedMs: 0,
    );
    recordingController.add(_recording);
    return take;
  }

  @override
  Future<InterruptedSessionView?> interruptedSession() async => _interrupted;

  @override
  Future<TakeView> interruptedFinalize() async {
    final interrupted = _interrupted;
    if (interrupted == null) {
      lastError = 'no recording is active';
      throw Exception(lastError);
    }
    _interrupted = null;
    final take = TakeView(
      title: 'take-20261022-2101',
      format: settings.defaultFormat,
      durationSecs: 42,
      audioPath: '/tmp/sessions/recovered.${settings.defaultFormat}',
    );
    finalizedTakes.add(take);
    return take;
  }

  @override
  Future<void> interruptedDiscard() async {
    if (_interrupted == null) {
      lastError = 'no recording is active';
      throw Exception(lastError);
    }
    _interrupted = null;
  }

  /// Test helper: a folder row.
  static TreeNodeView folder(
    String id,
    String name,
    List<String> path, {
    bool pending = false,
  }) =>
      TreeNodeView(
        id: id,
        name: name,
        kind: NodeKindView.folder,
        durationSecs: null,
        format: null,
        pending: pending,
        path: path,
      );

  /// Test helper: a recording row.
  static TreeNodeView file(
    String id,
    String name,
    List<String> path, {
    double? durationSecs,
    String? format,
    bool pending = false,
  }) =>
      TreeNodeView(
        id: id,
        name: name,
        kind: NodeKindView.file,
        durationSecs: durationSecs,
        format: format,
        pending: pending,
        path: path,
      );
}

/// Test helper: a conversation row.
ConversationView fakeConversation(
  String roomId,
  String name, {
  bool encrypted = true,
  bool encryptionKnown = true,
  String? lastSender,
  String? lastPreview,
  int? lastTimestampMs,
}) =>
    ConversationView(
      roomId: roomId,
      displayName: name,
      encrypted: encrypted,
      encryptionKnown: encryptionKnown,
      lastSender: lastSender,
      lastPreview: lastPreview,
      lastTimestampMs:
          lastTimestampMs == null ? null : BigInt.from(lastTimestampMs),
    );
