use std::time::Duration;

/// CPU frame timing. Camera and scene submission will be added with their systems.
#[derive(Clone, Copy, Debug)]
pub struct FrameContext {
    pub frame_index: u64,
    pub delta_time: Duration,
    pub elapsed_time: Duration,
}
