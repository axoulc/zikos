//! Delivery-state tracking for outgoing messages.
//!
//! matrix-sdk's send queue reports progress as
//! [`RoomSendQueueUpdate`]s with SDK-owned types; this module maps them
//! to Zikos's [`DeliveryState`] and enforces the state machine:
//!
//! ```text
//!            retry            send error
//!   sending ────────► failed ────────► (back to sending via retry)
//!      │
//!      └─ server accepted ─► sent (terminal)
//! ```

use std::collections::{HashMap, HashSet};
use std::sync::{Arc, Mutex, Weak};

use matrix_sdk::send_queue::{RoomSendQueueUpdate, SendQueueUpdate};
use tokio::sync::broadcast;

/// Delivery state of an outgoing message.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum DeliveryState {
    /// Queued or being transmitted.
    Sending,
    /// The homeserver accepted the event.
    Sent {
        /// Server-assigned event ID of the accepted message.
        event_id: String,
    },
    /// A send attempt failed; it will be retried when connectivity
    /// returns.
    Failed,
}

/// A delivery-state transition for one outgoing message, broadcast on
/// [`crate::Client::deliveries`].
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DeliveryEvent {
    /// Room the message belongs to.
    pub room_id: String,
    /// Send-queue transaction ID identifying the outgoing message (the
    /// value [`crate::Client::send_text`] returned).
    pub transaction_id: String,
    /// The new delivery state.
    pub state: DeliveryState,
}

/// The signals the SDK send queue can emit, in Zikos terms (adapted from
/// `RoomSendQueueUpdate` at the wiring site).
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum SendQueueSignal {
    New {
        transaction_id: String,
    },
    Error {
        transaction_id: String,
    },
    Retry {
        transaction_id: String,
    },
    Sent {
        transaction_id: String,
        event_id: String,
    },
}

impl SendQueueSignal {
    fn transaction_id(&self) -> &str {
        match self {
            SendQueueSignal::New { transaction_id }
            | SendQueueSignal::Error { transaction_id }
            | SendQueueSignal::Retry { transaction_id }
            | SendQueueSignal::Sent { transaction_id, .. } => transaction_id,
        }
    }
}

/// Translate an SDK send-queue update into our signal, if it concerns
/// delivery state.
pub(crate) fn signal_from_update(update: RoomSendQueueUpdate) -> Option<SendQueueSignal> {
    let signal = match update {
        RoomSendQueueUpdate::NewLocalEvent(echo) => SendQueueSignal::New {
            transaction_id: echo.transaction_id.to_string(),
        },
        RoomSendQueueUpdate::SendError { transaction_id, .. } => SendQueueSignal::Error {
            transaction_id: transaction_id.to_string(),
        },
        RoomSendQueueUpdate::RetryEvent { transaction_id } => SendQueueSignal::Retry {
            transaction_id: transaction_id.to_string(),
        },
        RoomSendQueueUpdate::SentEvent {
            transaction_id,
            event_id,
        } => SendQueueSignal::Sent {
            transaction_id: transaction_id.to_string(),
            event_id: event_id.to_string(),
        },
        _ => return None,
    };
    Some(signal)
}

/// The live send path: one global watcher over the SDK send queue feeding
/// the [`DeliveryTracker`] and the delivery event channel.
pub(crate) struct SendPath {
    tracker: Mutex<DeliveryTracker>,
    /// Serializes `send_text` per room so the transaction ID of the just
    /// enqueued message can be identified deterministically.
    room_locks: Mutex<HashMap<String, Arc<tokio::sync::Mutex<()>>>>,
    tx: broadcast::Sender<DeliveryEvent>,
}

impl SendPath {
    pub(crate) fn new() -> Self {
        let (tx, _) = broadcast::channel(256);
        Self {
            tracker: Mutex::new(DeliveryTracker::default()),
            room_locks: Mutex::new(HashMap::new()),
            tx,
        }
    }

    /// Receiver for delivery-state changes of outgoing messages.
    pub(crate) fn subscribe(&self) -> broadcast::Receiver<DeliveryEvent> {
        self.tx.subscribe()
    }

    pub(crate) fn room_lock(&self, room_id: &str) -> Arc<tokio::sync::Mutex<()>> {
        let mut locks = self.room_locks.lock().unwrap_or_else(|p| p.into_inner());
        locks
            .entry(room_id.to_owned())
            .or_insert_with(|| Arc::new(tokio::sync::Mutex::new(())))
            .clone()
    }

    /// Apply one signal; broadcast when the tracker accepted it.
    pub(crate) fn apply(&self, room_id: &str, signal: SendQueueSignal) {
        let Some(state) = self
            .tracker
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .observe(signal.clone())
        else {
            return;
        };
        let _ = self.tx.send(DeliveryEvent {
            room_id: room_id.to_owned(),
            transaction_id: signal.transaction_id().to_owned(),
            state,
        });
    }

    /// Seed the tracker from send-queue local echoes loaded from the
    /// store: `(transaction id, carried a past send error)`.
    pub(crate) fn restore_echoes(&self, echoes: impl IntoIterator<Item = (String, bool)>) {
        self.tracker
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .restore(echoes);
    }

    /// The transaction IDs of local echoes a room's queue currently holds
    /// (used by `send_text` to identify its own enqueue).
    pub(crate) fn known_transaction_ids(
        echoes: &[matrix_sdk::send_queue::LocalEcho],
    ) -> HashSet<String> {
        echoes
            .iter()
            .map(|e| e.transaction_id.to_string())
            .collect()
    }

    /// Spawn the global send-queue watcher. The task holds only a weak
    /// reference back to this path, so it dies with the client.
    pub(crate) fn watch(self: &Arc<Self>, client: &matrix_sdk::Client) {
        let mut updates = client.send_queue().subscribe();
        let path: Weak<Self> = Arc::downgrade(self);
        tokio::spawn(async move {
            loop {
                match updates.recv().await {
                    Ok(SendQueueUpdate { room_id, update }) => {
                        let Some(path) = path.upgrade() else { break };
                        if let Some(signal) = signal_from_update(update) {
                            path.apply(room_id.as_str(), signal);
                        }
                    }
                    // Slow consumer: drop lagged batch, keep listening.
                    Err(broadcast::error::RecvError::Lagged(_)) => continue,
                    // Send queue gone: client dropped.
                    Err(broadcast::error::RecvError::Closed) => break,
                }
            }
        });
    }
}

/// Tracks delivery states per send-queue transaction ID. Pure: no I/O, no
/// clocks, safe to unit-test exhaustively.
#[derive(Default)]
pub(crate) struct DeliveryTracker {
    states: HashMap<String, DeliveryState>,
}

impl DeliveryTracker {
    /// Apply one send-queue signal. Returns the new state, or `None` when
    /// the signal was ignored (e.g. an update for an already-sent
    /// message: `sent` is terminal).
    pub(crate) fn observe(&mut self, signal: SendQueueSignal) -> Option<DeliveryState> {
        use SendQueueSignal::*;
        match signal {
            New { transaction_id } => {
                let state = DeliveryState::Sending;
                self.states.insert(transaction_id, state.clone());
                Some(state)
            }
            Retry { transaction_id } => self.transition(transaction_id, DeliveryState::Sending),
            Error { transaction_id } => self.transition(transaction_id, DeliveryState::Failed),
            Sent {
                transaction_id,
                event_id,
            } => self.transition(transaction_id, DeliveryState::Sent { event_id }),
        }
    }

    /// The current state of one outgoing message, if known. Used by the
    /// unit tests; runtime consumers follow the event channel.
    #[cfg(test)]
    pub(crate) fn state(&self, transaction_id: &str) -> Option<&DeliveryState> {
        self.states.get(transaction_id)
    }

    /// Seed states from send-queue local echoes loaded from the store on
    /// restart: an echo carrying a past error is `Failed`, otherwise it
    /// is still `Sending`.
    pub(crate) fn restore(&mut self, echoes: impl IntoIterator<Item = (String, bool)>) {
        self.states
            .extend(echoes.into_iter().map(|(transaction_id, had_error)| {
                let state = if had_error {
                    DeliveryState::Failed
                } else {
                    DeliveryState::Sending
                };
                (transaction_id, state)
            }));
    }

    fn transition(&mut self, transaction_id: String, next: DeliveryState) -> Option<DeliveryState> {
        // `sent` is terminal: any late update for it is ignored.
        if matches!(
            self.states.get(&transaction_id),
            Some(DeliveryState::Sent { .. })
        ) {
            return None;
        }
        self.states.insert(transaction_id, next.clone());
        Some(next)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn txn() -> String {
        "txn-1".to_owned()
    }

    #[test]
    fn new_starts_sending() {
        let mut t = DeliveryTracker::default();
        assert_eq!(
            t.observe(SendQueueSignal::New {
                transaction_id: txn()
            }),
            Some(DeliveryState::Sending)
        );
        assert_eq!(t.state(&txn()), Some(&DeliveryState::Sending));
    }

    #[test]
    fn success_path_is_sending_then_sent() {
        let mut t = DeliveryTracker::default();
        t.observe(SendQueueSignal::New {
            transaction_id: txn(),
        });
        assert_eq!(
            t.observe(SendQueueSignal::Sent {
                transaction_id: txn(),
                event_id: "$ev".to_owned(),
            }),
            Some(DeliveryState::Sent {
                event_id: "$ev".to_owned()
            })
        );
    }

    #[test]
    fn sent_is_terminal() {
        let mut t = DeliveryTracker::default();
        t.observe(SendQueueSignal::New {
            transaction_id: txn(),
        });
        t.observe(SendQueueSignal::Sent {
            transaction_id: txn(),
            event_id: "$ev".to_owned(),
        });
        assert_eq!(
            t.observe(SendQueueSignal::Error {
                transaction_id: txn()
            }),
            None,
            "updates after sent must be ignored"
        );
        assert_eq!(
            t.state(&txn()),
            Some(&DeliveryState::Sent {
                event_id: "$ev".to_owned()
            })
        );
    }

    #[test]
    fn error_fails_then_retry_resumes_and_sent_completes() {
        let mut t = DeliveryTracker::default();
        t.observe(SendQueueSignal::New {
            transaction_id: txn(),
        });
        assert_eq!(
            t.observe(SendQueueSignal::Error {
                transaction_id: txn()
            }),
            Some(DeliveryState::Failed)
        );
        assert_eq!(
            t.observe(SendQueueSignal::Retry {
                transaction_id: txn()
            }),
            Some(DeliveryState::Sending)
        );
        assert_eq!(
            t.observe(SendQueueSignal::Sent {
                transaction_id: txn(),
                event_id: "$ev2".to_owned(),
            }),
            Some(DeliveryState::Sent {
                event_id: "$ev2".to_owned()
            })
        );
    }

    #[test]
    fn restore_marks_wedged_echoes_failed_and_others_sending() {
        let mut t = DeliveryTracker::default();
        t.restore([
            ("ok-txn".to_owned(), false),
            ("wedged-txn".to_owned(), true),
        ]);
        assert_eq!(t.state("ok-txn"), Some(&DeliveryState::Sending));
        assert_eq!(t.state("wedged-txn"), Some(&DeliveryState::Failed));
    }

    #[test]
    fn update_for_unknown_txn_is_tracked() {
        // After a restart mid-flight, updates can precede any New signal.
        let mut t = DeliveryTracker::default();
        assert_eq!(
            t.observe(SendQueueSignal::Sent {
                transaction_id: "late".to_owned(),
                event_id: "$e".to_owned(),
            }),
            Some(DeliveryState::Sent {
                event_id: "$e".to_owned()
            })
        );
    }
}
