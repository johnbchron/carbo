use std::sync::mpsc;

use crate::event::Event;

/// Enables sending [`Event`]s to the [`App`](crate::app::App).
#[derive(Clone, Debug)]
pub struct EventSender {
  event_tx: mpsc::Sender<Event>,
}

impl EventSender {
  pub fn new(event_tx: mpsc::Sender<Event>) -> Self { Self { event_tx } }

  /// Sends an [`Event`].
  pub fn event(&self, event: Event) {
    self
      .event_tx
      .send(event)
      .expect("failed to send event: app thread has exited");
  }

  /// Attempts to send an [`Event`].
  pub fn try_event(&self, event: Event) -> Result<(), ()> {
    self.event_tx.send(event).map_err(|_| ())
  }
}
