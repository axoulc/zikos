/// Production [ZikosApi]: pure 1:1 delegation to the generated
/// flutter_rust_bridge bindings. No logic lives here (by design).
library;

import 'zikos_api.dart';

import '../rust/api/app.dart' as rust_app;
import '../rust/api/conversations.dart' as rust_conversations;
import '../rust/api/files.dart' as rust_files;
import '../rust/api/recorder.dart' as rust_recorder;
import '../rust/api/settings.dart' as rust_settings;

/// Delegates every call to the bridge-generated functions.
class FrbZikosApi implements ZikosApi {
  /// Creates the delegator; call `RustLib.init()` before use (main does).
  const FrbZikosApi();

  @override
  Future<AppSnapshot> init({required String appDir}) =>
      rust_app.init(appDir: appDir);

  @override
  Stream<SessionView> sessionStream() => rust_app.sessionStream();

  @override
  Future<SettingsView> settingsGet() => rust_settings.settingsGet();

  @override
  Future<SettingsView> settingsSave({
    required String homeserverUrl,
    required String blobServerUrl,
    required String defaultFormat,
  }) =>
      rust_settings.settingsSave(
        homeserverUrl: homeserverUrl,
        blobServerUrl: blobServerUrl,
        defaultFormat: defaultFormat,
      );

  @override
  Future<List<ConversationView>> conversations() =>
      rust_conversations.conversations();

  @override
  Stream<List<ConversationView>> conversationsStream() =>
      rust_conversations.conversationsStream();

  @override
  Future<List<ConversationView>> refreshConversations() =>
      rust_conversations.refreshConversations();

  @override
  Future<void> selectConversation({required String roomId}) =>
      rust_conversations.selectConversation(roomId: roomId);

  @override
  Future<ConversationView?> currentConversation() =>
      rust_conversations.currentConversation();

  @override
  Future<List<TreeNodeView>> folderListing({String? folderId}) =>
      rust_files.folderListing(folderId: folderId);

  @override
  Future<RecordingStateView> recordingState() =>
      rust_recorder.recordingState();

  @override
  Stream<RecordingStateView> recordingStateStream() =>
      rust_recorder.recordingStateStream();

  @override
  Future<void> recordStart() => rust_recorder.recordStart();

  @override
  Future<void> recordPause() => rust_recorder.recordPause();

  @override
  Future<void> recordResume() => rust_recorder.recordResume();

  @override
  Future<TakeView> recordStop() => rust_recorder.recordStop();

  @override
  Future<InterruptedSessionView?> interruptedSession() =>
      rust_recorder.interruptedSession();

  @override
  Future<TakeView> interruptedFinalize() =>
      rust_recorder.interruptedFinalize();

  @override
  Future<void> interruptedDiscard() => rust_recorder.interruptedDiscard();
}
