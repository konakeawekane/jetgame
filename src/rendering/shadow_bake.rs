//! Reserved for one-time level shadow baking after the host selects the sun.
//!
//! Construct terrain shadow volumes, classify terrain vertices and ground cover,
//! and retain the resulting static visibility data for normal rendering.
//! Construction data should be releasable once baking completes.
//!
//! No algorithm or baked-value encoding is selected yet. This module is not
//! called by the frame loop. Optional terrain shadows on moving aircraft and
//! aircraft local shadow maps will be designed separately if budgets permit.
