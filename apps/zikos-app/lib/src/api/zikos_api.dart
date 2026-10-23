/// The seam between the UI and the Rust core.
///
/// Screens depend on this abstract facade — never on the generated
/// flutter_rust_bridge bindings — so every screen is widget-testable
/// with [FakeZikosApi] and the production [FrbZikosApi] stays a pure
/// delegator (specs/platform-app.md: UI bound to the Rust core).
///
/// Model classes are the bridge-generated ones, used directly as view
/// models; Dart performs no domain computation beyond display formatting.
library;

import '../rust/api/app.dart' show AppSnapshot, SessionView;
import '../rust/api/conversations.dart' show ConversationView;
import '../rust/api/files.dart' show TreeNodeView;
import '../rust/api/recorder.dart'
    show
        InterruptedSessionView,
        RecordingStateView,
        RecordingStatus,
        TakeView;
import '../rust/api/settings.dart' show SettingsView;

// Screens import model types through this facade, never from the
// generated files directly.
export '../rust/api/app.dart' show AppSnapshot, SessionView;
export '../rust/api/conversations.dart' show ConversationView;
export '../rust/api/files.dart' show NodeKindView, TreeNodeView;
export '../rust/api/recorder.dart'
    show InterruptedSessionView, RecordingStateView, RecordingStatus, TakeView;
export '../rust/api/settings.dart' show SettingsView;

/// Everything the UI can ask the Rust core for.
abstract class ZikosApi {
  /// Initialize the Rust state rooted at [appDir] (documents dir).
  Future<AppSnapshot> init({required String appDir});

  /// Session snapshot stream (signed-out until a login flow exists).
  Stream<SessionView> sessionStream();

  /// Effective settings (defaults when never configured).
  Future<SettingsView> settingsGet();

  /// Validate and persist settings; throws on invalid values.
  Future<SettingsView> settingsSave({
    required String homeserverUrl,
    required String blobServerUrl,
    required String defaultFormat,
  });

  /// Cached conversation list (empty while signed out).
  Future<List<ConversationView>> conversations();

  /// The conversation list as a stream: current list immediately, then
  /// the full list whenever it changes.
  Stream<List<ConversationView>> conversationsStream();

  /// Refresh the list from the Matrix session.
  Future<List<ConversationView>> refreshConversations();

  /// Select the conversation Files and Record operate on.
  Future<void> selectConversation({required String roomId});

  /// The selected conversation, if any.
  Future<ConversationView?> currentConversation();

  /// One folder listing of the current conversation's tree
  /// (`null` = root).
  Future<List<TreeNodeView>> folderListing({String? folderId});

  /// Current recorder state.
  Future<RecordingStateView> recordingState();

  /// Recorder state stream (emits on every change).
  Stream<RecordingStateView> recordingStateStream();

  /// Start a take; throws when one is active or an interrupted session
  /// is pending.
  Future<void> recordStart();

  /// Pause the active take.
  Future<void> recordPause();

  /// Resume the paused take.
  Future<void> recordResume();

  /// Stop and finalize the active take.
  Future<TakeView> recordStop();

  /// The interrupted session awaiting finalize-or-discard, if any.
  Future<InterruptedSessionView?> interruptedSession();

  /// Finalize the interrupted session.
  Future<TakeView> interruptedFinalize();

  /// Discard the interrupted session.
  Future<void> interruptedDiscard();
}

/// Convenience: whether a recorder state means an active take.
extension RecordingActive on RecordingStateView {
  /// Idle means no take; recording and paused both show the indicator.
  bool get isActive => status != RecordingStatus.idle;
}
