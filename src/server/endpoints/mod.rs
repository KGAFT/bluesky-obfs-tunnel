//! Owns endpoint tasks, bounded input queues, and ordered endpoint events.
mod relay;

use crate::{StreamLimits, protocol::ForwardProtocol, server::connect_policy::ConnectPolicy};
use relay::RelayContext;
use std::collections::HashMap;
use tokio::{
    sync::mpsc,
    task::{AbortHandle, JoinSet},
};
use tokio_util::bytes::Bytes;

pub enum EndpointEvent {
    Connected(u64),
    Data(u64, Bytes),
    ReadClosed(u64),
    Closed(u64),
    Failed(u64),
}

enum Input {
    Data(Bytes),
    ShutdownWrite,
}

struct Endpoint {
    sender: mpsc::Sender<Input>,
    task: AbortHandle,
    generation: u64,
    protocol: ForwardProtocol,
    write_closed: bool,
}

/// Owns endpoint tasks; dropping a session cancels both connects and relays.
pub struct OneToManyStreams {
    endpoints: HashMap<u64, Endpoint>,
    tasks: JoinSet<()>,
    event_tx: mpsc::Sender<(u64, EndpointEvent)>,
    event_rx: mpsc::Receiver<(u64, EndpointEvent)>,
    generation: u64,
    limits: StreamLimits,
    policy: ConnectPolicy,
}

impl OneToManyStreams {
    pub fn new(limits: StreamLimits, policy: ConnectPolicy) -> Self {
        let (event_tx, event_rx) = mpsc::channel(limits.endpoint_event_queue_depth.max(1));
        Self {
            endpoints: HashMap::new(),
            tasks: JoinSet::new(),
            event_tx,
            event_rx,
            generation: 0,
            limits,
            policy,
        }
    }

    /// Original HTTP proxy behavior: target EOF closes the endpoint.
    pub fn begin_connect(&mut self, id: u64, destination: String) -> bool {
        self.begin(id, destination, ForwardProtocol::Tcp, false)
    }

    pub fn begin_forward(
        &mut self,
        id: u64,
        destination: String,
        protocol: ForwardProtocol,
    ) -> bool {
        self.begin(id, destination, protocol, true)
    }

    fn begin(
        &mut self,
        id: u64,
        destination: String,
        protocol: ForwardProtocol,
        multiplexed: bool,
    ) -> bool {
        if self.endpoints.contains_key(&id) {
            return false;
        }
        let Some(generation) = self.generation.checked_add(1) else {
            return false;
        };
        self.generation = generation;
        let (sender, receiver) = mpsc::channel(self.limits.max_pending_frames.max(1));
        let events = self.event_tx.clone();
        let policy = self.policy.clone();
        let task = self.tasks.spawn(async move {
            let destination_label = destination.clone();
            let context = RelayContext {
                id,
                generation,
                events,
                multiplexed,
            };
            let result = match protocol {
                ForwardProtocol::Tcp => context.tcp(destination, policy, receiver).await,
                ForwardProtocol::Udp => context.udp(destination, policy, receiver).await,
            };
            // The same event queue orders response data before terminal events.
            let event = match result {
                Ok(()) => EndpointEvent::Closed(id),
                Err(error) => {
                    eprintln!(
                        "Forward {protocol:?} endpoint {id} to {destination_label} failed: {error}"
                    );
                    EndpointEvent::Failed(id)
                }
            };
            let _ = context.emit(event).await;
        });
        self.endpoints.insert(
            id,
            Endpoint {
                sender,
                task,
                generation,
                protocol,
                write_closed: false,
            },
        );
        true
    }

    pub fn protocol(&self, id: u64) -> Option<ForwardProtocol> {
        self.endpoints.get(&id).map(|endpoint| endpoint.protocol)
    }

    pub fn close(&mut self, id: u64) {
        if let Some(endpoint) = self.endpoints.remove(&id) {
            endpoint.task.abort();
        }
    }

    pub fn send_to(&mut self, id: u64, data: Bytes) -> bool {
        let Some(endpoint) = self.endpoints.get(&id) else {
            return false;
        };
        if !endpoint.write_closed && endpoint.sender.try_send(Input::Data(data)).is_ok() {
            return true;
        }
        // A full queue fails this endpoint, leaving the other forwards running.
        self.close(id);
        false
    }

    pub fn shutdown_write(&mut self, id: u64) -> bool {
        let Some(endpoint) = self.endpoints.get_mut(&id) else {
            return false;
        };
        if endpoint.protocol != ForwardProtocol::Tcp {
            return false;
        }
        if endpoint.write_closed {
            return true;
        }
        if endpoint.sender.try_send(Input::ShutdownWrite).is_ok() {
            endpoint.write_closed = true;
            return true;
        }
        self.close(id);
        false
    }

    pub async fn receive(&mut self) -> EndpointEvent {
        loop {
            tokio::select! {
                event = self.event_rx.recv() => {
                    let (generation, event) = event.expect("session owns the event sender");
                    let id = match &event {
                        EndpointEvent::Connected(id) | EndpointEvent::Data(id, _)
                        | EndpointEvent::ReadClosed(id) | EndpointEvent::Closed(id)
                        | EndpointEvent::Failed(id) => *id,
                    };
                    // Discard queued events belonging to aborted/replaced endpoints.
                    if !self.endpoints.get(&id).is_some_and(|e| e.generation == generation) { continue; }
                    if matches!(event, EndpointEvent::Closed(_) | EndpointEvent::Failed(_)) {
                        self.endpoints.remove(&id);
                    }
                    return event;
                }
                _ = self.tasks.join_next(), if !self.tasks.is_empty() => {}
            }
        }
    }
}
