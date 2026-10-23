/// Conversations: the account's conversation list — rows with
/// encryption glyph, snippet and time, live updates, tap to select the
/// current conversation (specs/platform-app.md, "conversations screen").
library;

import 'dart:async';

import 'package:flutter/material.dart';
import 'package:provider/provider.dart';

import '../api/zikos_api.dart';

/// The Conversations destination.
class ConversationsScreen extends StatefulWidget {
  /// Creates the screen.
  const ConversationsScreen({super.key});

  @override
  State<ConversationsScreen> createState() => _ConversationsScreenState();
}

class _ConversationsScreenState extends State<ConversationsScreen> {
  List<ConversationView> _conversations = [];
  bool _signedIn = false;
  String? _currentRoomId;
  StreamSubscription<List<ConversationView>>? _conversationsSub;
  StreamSubscription<SessionView>? _sessionSub;

  @override
  void initState() {
    super.initState();
    final api = context.read<ZikosApi>();
    _conversationsSub = api.conversationsStream().listen((list) {
      if (!mounted) return;
      setState(() => _conversations = list);
    });
    _sessionSub = api.sessionStream().listen((session) {
      if (!mounted) return;
      setState(() => _signedIn = session.signedIn);
    });
    // Pull once too: cheap, and covers anything the stream's initial
    // snapshot might have raced.
    _refresh(api);
    _syncCurrent(api);
  }

  Future<void> _refresh(ZikosApi api) async {
    try {
      await api.refreshConversations();
    } catch (_) {
      // Offline or signed out: the stream's snapshot is what we render.
    }
  }

  Future<void> _syncCurrent(ZikosApi api) async {
    final current = await api.currentConversation();
    if (!mounted) return;
    setState(() => _currentRoomId = current?.roomId);
  }

  @override
  void dispose() {
    _conversationsSub?.cancel();
    _sessionSub?.cancel();
    super.dispose();
  }

  @override
  Widget build(BuildContext context) {
    if (!_signedIn) {
      return const KeyedSubtree(
        key: ValueKey('conversations-screen'),
        child: _SignedOut(),
      );
    }
    return KeyedSubtree(
      key: const ValueKey('conversations-screen'),
      child: ListView.builder(
        key: const ValueKey('conversations-list'),
        itemCount: _conversations.length,
        itemBuilder: (context, index) => _row(context, _conversations[index]),
      ),
    );
  }

  Widget _row(BuildContext context, ConversationView conversation) {
    final theme = Theme.of(context);
    final isCurrent = conversation.roomId == _currentRoomId;
    return ListTile(
      onTap: () async {
        await context.read<ZikosApi>().selectConversation(
              roomId: conversation.roomId,
            );
        setState(() => _currentRoomId = conversation.roomId);
      },
      leading: CircleAvatar(child: Text(_initials(conversation.displayName))),
      title: Row(
        children: [
          Flexible(
            child: Text(
              conversation.displayName,
              overflow: TextOverflow.ellipsis,
              style: isCurrent ? const TextStyle(fontWeight: FontWeight.w700) : null,
            ),
          ),
          // The current conversation carries a cue beyond the tint: an
          // accent edge marker (design D10).
          if (isCurrent)
            Padding(
              padding: const EdgeInsets.only(left: 8),
              child: Container(
                key: const ValueKey('current-conversation-marker'),
                width: 4,
                height: 18,
                decoration: BoxDecoration(
                  color: theme.colorScheme.primary,
                  borderRadius: BorderRadius.circular(2),
                ),
              ),
            ),
        ],
      ),
      subtitle: conversation.lastPreview == null
          ? null
          : Text(
              '${conversation.lastSender ?? ''}: ${conversation.lastPreview}',
              maxLines: 1,
              overflow: TextOverflow.ellipsis,
            ),
      trailing: Column(
        mainAxisAlignment: MainAxisAlignment.center,
        crossAxisAlignment: CrossAxisAlignment.end,
        children: [
          if (conversation.lastTimestampMs != null)
            Text(_clockLabel(conversation.lastTimestampMs!)),
          _encryptionGlyph(conversation),
        ],
      ),
      selected: isCurrent,
      selectedTileColor: theme.colorScheme.secondaryContainer.withAlpha(120),
    );
  }

  /// Distinct glyphs, not just colors (design D10): closed lock, open
  /// lock, question mark.
  Widget _encryptionGlyph(ConversationView conversation) {
    const size = 16.0;
    if (!conversation.encryptionKnown) {
      return const Icon(
        key: ValueKey('encryption-unknown'),
        Icons.help_outline,
        size: size,
        color: Colors.grey,
      );
    }
    if (conversation.encrypted) {
      return Icon(
        key: const ValueKey('encryption-encrypted'),
        Icons.lock,
        size: size,
        color: Colors.green.shade700,
      );
    }
    return Icon(
      key: const ValueKey('encryption-unencrypted'),
      Icons.lock_open,
      size: size,
      color: Colors.amber.shade800,
    );
  }

  String _initials(String name) {
    final words =
        name.trim().split(RegExp(r'\s+')).where((w) => w.isNotEmpty).toList();
    if (words.isEmpty) return '?';
    if (words.length == 1) {
      return words.first.characters.take(2).toString().toUpperCase();
    }
    return (words.first.characters.first + words.last.characters.first)
        .toUpperCase();
  }

  String _clockLabel(BigInt timestampMs) {
    final time = DateTime.fromMillisecondsSinceEpoch(timestampMs.toInt());
    String two(int n) => n.toString().padLeft(2, '0');
    return '${two(time.hour)}:${two(time.minute)}';
  }
}

class _SignedOut extends StatelessWidget {
  const _SignedOut();

  @override
  Widget build(BuildContext context) {
    return Center(
      key: const ValueKey('signed-out-state'),
      child: SingleChildScrollView(
        padding: const EdgeInsets.all(24),
        child: Column(
          mainAxisAlignment: MainAxisAlignment.center,
          children: [
            Icon(
              Icons.lock_outline,
              size: 48,
              color: Theme.of(context).colorScheme.primary,
            ),
            const SizedBox(height: 12),
            Text(
              'Not signed in',
              style: Theme.of(context).textTheme.titleMedium,
            ),
            const SizedBox(height: 8),
            const Text(
              'Zikos chats end-to-end encrypted over Matrix. Configure your '
              'homeserver in Settings and sign in to see your conversations.',
              textAlign: TextAlign.center,
            ),
          ],
        ),
      ),
    );
  }
}
