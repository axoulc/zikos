/// Files: the current conversation's recording tree — sorted folder
/// listing with duration/format chips and pending markers, breadcrumb
/// navigation, and the recording details sheet
/// (specs/platform-app.md, "tree browser screen").
library;

import 'package:flutter/material.dart';
import 'package:provider/provider.dart';

import '../api/zikos_api.dart';

/// One visited folder on the navigation stack: its id (for the next
/// listing) and its name (for the breadcrumb).
class _VisitedFolder {
  const _VisitedFolder({required this.id, required this.name});

  final String id;
  final String name;
}

/// The Files destination.
class FilesScreen extends StatefulWidget {
  /// Creates the screen.
  const FilesScreen({super.key});

  @override
  State<FilesScreen> createState() => _FilesScreenState();
}

class _FilesScreenState extends State<FilesScreen> {
  final List<_VisitedFolder> _stack = [];
  List<TreeNodeView> _rows = [];
  ConversationView? _conversation;
  bool _loading = true;
  String? _error;

  @override
  void initState() {
    super.initState();
    _reload();
  }

  Future<void> _reload() async {
    final api = context.read<ZikosApi>();
    final conversation = await api.currentConversation();
    if (!mounted) return;
    if (conversation == null) {
      setState(() {
        _conversation = null;
        _loading = false;
      });
      return;
    }
    final folderId = _stack.isEmpty ? null : _stack.last.id;
    try {
      final rows = await api.folderListing(folderId: folderId);
      if (!mounted) return;
      setState(() {
        _conversation = conversation;
        _rows = rows;
        _error = null;
        _loading = false;
      });
    } catch (e) {
      // Offline: the cached tree is what the bridge could serve; show the
      // failure rather than pretending the folder is empty.
      if (!mounted) return;
      setState(() {
        _conversation = conversation;
        _error = e.toString();
        _loading = false;
      });
    }
  }

  void _openFolder(_VisitedFolder folder) {
    setState(() => _stack.add(folder));
    _reload();
  }

  void _jumpTo(int depth) {
    // depth 0 = tree root; n = inside the n-th visited folder.
    setState(() => _stack.removeRange(depth, _stack.length));
    _reload();
  }

  Future<void> _showDetails(TreeNodeView node) async {
    await showModalBottomSheet<void>(
      context: context,
      showDragHandle: true,
      builder: (context) =>
          _DetailsSheet(node: node, conversation: _conversation),
    );
  }

  @override
  Widget build(BuildContext context) {
    return KeyedSubtree(
      key: const ValueKey('files-screen'),
      child: _build(context),
    );
  }

  Widget _build(BuildContext context) {
    if (_conversation == null) {
      return const _NoConversation();
    }
    return Column(
      crossAxisAlignment: CrossAxisAlignment.stretch,
      children: [
        Padding(
          padding: const EdgeInsets.fromLTRB(16, 12, 16, 4),
          child: Chip(
            avatar: const Icon(Icons.lock, size: 14),
            label: Text(_conversation!.displayName),
            visualDensity: VisualDensity.compact,
          ),
        ),
        _breadcrumb(context),
        if (_error != null)
          Padding(
            padding: const EdgeInsets.all(12),
            child: Text(
              'Could not list this folder: $_error',
              style: TextStyle(color: Theme.of(context).colorScheme.error),
            ),
          ),
        Expanded(
          child: _loading
              ? const Center(child: CircularProgressIndicator())
              : RefreshIndicator(
                  onRefresh: _reload,
                  child: ListView.builder(
                    itemCount: _rows.length,
                    itemBuilder: (context, index) => _row(_rows[index]),
                  ),
                ),
        ),
      ],
    );
  }

  Widget _breadcrumb(BuildContext context) {
    final crumbs = <Widget>[_crumb(context, 'Tree', 0)];
    for (final (index, folder) in _stack.indexed) {
      crumbs.add(const Padding(
        padding: EdgeInsets.symmetric(horizontal: 2),
        child: Icon(Icons.chevron_right, size: 16),
      ));
      crumbs.add(
        _crumb(
          context,
          folder.name,
          index + 1,
          last: index == _stack.length - 1,
        ),
      );
    }
    return SingleChildScrollView(
      scrollDirection: Axis.horizontal,
      padding: const EdgeInsets.symmetric(horizontal: 16, vertical: 4),
      child: Row(children: crumbs),
    );
  }

  Widget _crumb(BuildContext context, String label, int depth,
      {bool last = false}) {
    final theme = Theme.of(context);
    return InkWell(
      key: ValueKey('crumb-$label'),
      onTap: last ? null : () => _jumpTo(depth),
      child: Text(
        label,
        style: last
            ? TextStyle(
                fontWeight: FontWeight.w700,
                color: theme.colorScheme.onSurface,
              )
            : TextStyle(color: theme.colorScheme.primary),
      ),
    );
  }

  Widget _row(TreeNodeView node) {
    final theme = Theme.of(context);
    final isFolder = node.kind == NodeKindView.folder;
    return ListTile(
      key: ValueKey('tree-node-${node.id}'),
      onTap: () => isFolder
          ? _openFolder(_VisitedFolder(id: node.id, name: node.name))
          : _showDetails(node),
      leading: Icon(
        isFolder ? Icons.folder : Icons.audio_file_outlined,
        color: isFolder
            ? theme.colorScheme.primary
            : theme.colorScheme.onSurfaceVariant,
      ),
      title: Text(node.name, overflow: TextOverflow.ellipsis),
      subtitle: !isFolder && node.durationSecs != null
          ? Text(_clock(node.durationSecs!))
          : null,
      trailing: isFolder
          ? null
          : Wrap(
              spacing: 6,
              crossAxisAlignment: WrapCrossAlignment.center,
              children: [
                if (node.pending)
                  Container(
                    key: const ValueKey('pending-marker'),
                    padding: const EdgeInsets.symmetric(
                      horizontal: 6,
                      vertical: 2,
                    ),
                    decoration: BoxDecoration(
                      color: theme.colorScheme.tertiaryContainer,
                      borderRadius: BorderRadius.circular(6),
                    ),
                    child: Row(
                      mainAxisSize: MainAxisSize.min,
                      children: [
                        Icon(
                          Icons.sync,
                          size: 12,
                          color: theme.colorScheme.onTertiaryContainer,
                        ),
                        const SizedBox(width: 3),
                        Text(
                          'pending',
                          style: TextStyle(
                            fontSize: 11,
                            color: theme.colorScheme.onTertiaryContainer,
                          ),
                        ),
                      ],
                    ),
                  ),
                if (node.format != null)
                  Container(
                    padding: const EdgeInsets.symmetric(
                      horizontal: 6,
                      vertical: 2,
                    ),
                    decoration: BoxDecoration(
                      color: theme.colorScheme.secondaryContainer,
                      borderRadius: BorderRadius.circular(6),
                    ),
                    child: Text(
                      node.format!.toUpperCase(),
                      style: TextStyle(
                        fontSize: 11,
                        fontWeight: FontWeight.w700,
                        color: theme.colorScheme.onSecondaryContainer,
                      ),
                    ),
                  ),
              ],
            ),
    );
  }

  String _clock(double seconds) {
    final total = seconds.round();
    final m = (total ~/ 60).toString().padLeft(2, '0');
    final s = (total % 60).toString().padLeft(2, '0');
    return '$m:$s';
  }
}

class _NoConversation extends StatelessWidget {
  const _NoConversation();

  @override
  Widget build(BuildContext context) {
    return Center(
      key: const ValueKey('no-conversation-state'),
      child: Padding(
        padding: const EdgeInsets.all(24),
        child: Column(
          mainAxisAlignment: MainAxisAlignment.center,
          children: [
            Icon(
              Icons.folder_open,
              size: 48,
              color: Theme.of(context).colorScheme.primary,
            ),
            const SizedBox(height: 12),
            Text(
              'No conversation selected',
              style: Theme.of(context).textTheme.titleMedium,
            ),
            const SizedBox(height: 8),
            const Text(
              'Pick a conversation in the Conversations tab to browse its '
              'shared recording tree.',
              textAlign: TextAlign.center,
            ),
          ],
        ),
      ),
    );
  }
}

class _DetailsSheet extends StatelessWidget {
  const _DetailsSheet({required this.node, required this.conversation});

  final TreeNodeView node;
  final ConversationView? conversation;

  @override
  Widget build(BuildContext context) {
    final theme = Theme.of(context);
    return Padding(
      key: const ValueKey('recording-details-sheet'),
      padding: const EdgeInsets.fromLTRB(24, 0, 24, 32),
      child: Column(
        mainAxisSize: MainAxisSize.min,
        crossAxisAlignment: CrossAxisAlignment.start,
        children: [
          Row(
            children: [
              Icon(
                Icons.audio_file_outlined,
                color: theme.colorScheme.primary,
              ),
              const SizedBox(width: 10),
              Expanded(
                child: Text(
                  node.name,
                  style: theme.textTheme.titleMedium,
                  overflow: TextOverflow.ellipsis,
                ),
              ),
            ],
          ),
          const SizedBox(height: 14),
          _fact(theme, 'Duration',
              node.durationSecs == null ? '—' : _clock(node.durationSecs!)),
          _fact(theme, 'Format', (node.format ?? '—').toUpperCase()),
          _fact(
            theme,
            'Location',
            '${conversation?.displayName ?? ''} › ${node.path.join(' › ')}',
          ),
          if (node.pending)
            Padding(
              padding: const EdgeInsets.only(top: 8),
              child: Text(
                'Still syncing to the conversation',
                style: TextStyle(color: theme.colorScheme.tertiary),
              ),
            ),
        ],
      ),
    );
  }

  Widget _fact(ThemeData theme, String label, String value) {
    return Padding(
      padding: const EdgeInsets.symmetric(vertical: 4),
      child: Row(
        crossAxisAlignment: CrossAxisAlignment.start,
        children: [
          SizedBox(
            width: 84,
            child: Text(label, style: theme.textTheme.bodySmall),
          ),
          Expanded(child: Text(value)),
        ],
      ),
    );
  }

  String _clock(double seconds) {
    final total = seconds.round();
    final m = (total ~/ 60).toString().padLeft(2, '0');
    final s = (total % 60).toString().padLeft(2, '0');
    return '$m:$s';
  }
}
