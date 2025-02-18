#![allow(missing_docs)]

mod manager;
mod migration;
#[cfg(test)]
mod tests;

pub use manager::*;
pub use migration::*;

use crate::spec::SpecId;

/// Fork is a wrapper struct that contains spec id and it's activation height
#[derive(Debug, Clone, Copy)]
pub struct Fork {
    /// Spec id for this fork
    pub spec_id: SpecId,
    /// Height to activate this spec
    pub activation_height: u64,
}

impl Fork {
    /// Creates new Fork instance
    pub const fn new(spec_id: SpecId, activation_height: u64) -> Self {
        Self {
            spec_id,
            activation_height,
        }
    }
}
