//! Queue capacities shared by client and server sessions.

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct StreamLimits {
    pub endpoint_event_queue_depth: usize,
    pub max_pending_frames: usize,
    pub sink_queue_depth: usize,
}

impl Default for StreamLimits {
    fn default() -> Self {
        Self {
            endpoint_event_queue_depth: 256,
            max_pending_frames: 64,
            sink_queue_depth: 256,
        }
    }
}
