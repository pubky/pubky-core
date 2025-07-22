use std::collections::HashMap;

use axum::body::Bytes;
use tokio::sync::oneshot;

/// A list of waiting producers and consumers.
#[derive(Default)]
pub struct WaitingList {
    pub pending_producers: HashMap<String, WaitingProducer>,
    pub pending_consumers: HashMap<String, WaitingConsumer>,
}

impl WaitingList {
    pub fn remove_producer(&mut self, id: &str) -> Option<WaitingProducer> {
        self.pending_producers.remove(id)
    }

    pub fn insert_producer(&mut self, id: &str, body: Bytes) -> oneshot::Receiver<()> {
        let (producer, completion_receiver) = WaitingProducer::new(body);
        self.pending_producers.insert(id.to_string(), producer);
        completion_receiver
    }

    pub fn remove_consumer(&mut self, id: &str) -> Option<WaitingConsumer> {
        self.pending_consumers.remove(id)
    }

    pub fn insert_consumer(&mut self, id: &str) -> oneshot::Receiver<Bytes> {
        let (consumer, message_receiver) = WaitingConsumer::new();
        self.pending_consumers.insert(id.to_string(), consumer);
        message_receiver
    }
}

/// A producer that is waiting for a consumer to request data.
pub struct WaitingProducer {
    /// The payload of the producer
    pub body: Bytes,
    /// The sender to notify the producer that the request has been resolved.
    pub completion: oneshot::Sender<()>,
}

impl WaitingProducer {
    fn new(body: Bytes) -> (Self, oneshot::Receiver<()>) {
        let (completion_sender, completion_receiver) = oneshot::channel();
        (
            Self {
                body,
                completion: completion_sender,
            },
            completion_receiver,
        )
    }
}

/// A consumer that is waiting for a producer to send data.
pub struct WaitingConsumer {
    /// The sender to notify the consumer that the request has been resolved.
    pub message_sender: oneshot::Sender<Bytes>,
}

impl WaitingConsumer {
    fn new() -> (Self, oneshot::Receiver<Bytes>) {
        let (message_sender, message_receiver) = oneshot::channel();
        (Self { message_sender }, message_receiver)
    }
}
