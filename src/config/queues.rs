use super::validation::positive;
use crate::StreamLimits;
use serde::{Deserialize, Serialize};

#[derive(Serialize, Deserialize, Debug, Clone, PartialEq)]
#[serde(default, deny_unknown_fields)]
pub struct QueueConfig {
    pub endpoint_event_queue_depth: u32,
    pub max_pending_frames: u32,
    pub sink_queue_depth: u32,
}

impl Default for QueueConfig {
    fn default() -> Self {
        Self {
            endpoint_event_queue_depth: 256,
            max_pending_frames: 64,
            sink_queue_depth: 256,
        }
    }
}

impl QueueConfig {
    pub fn limits(&self) -> StreamLimits {
        StreamLimits {
            endpoint_event_queue_depth: self.endpoint_event_queue_depth.max(1) as usize,
            max_pending_frames: self.max_pending_frames.max(1) as usize,
            sink_queue_depth: self.sink_queue_depth.max(1) as usize,
        }
    }

    pub(super) fn validate(&self, problems: &mut Vec<String>) {
        for (name, value) in [
            (
                "endpoint_event_queue_depth",
                self.endpoint_event_queue_depth,
            ),
            ("max_pending_frames", self.max_pending_frames),
            ("sink_queue_depth", self.sink_queue_depth),
        ] {
            positive(name, value.into(), problems);
        }
    }
}
