//! Read-only local provider discovery for Eagle and DaVinci Resolve.
//!
//! The production runtimes expose only fixed discovery and read operations. They do
//! not accept a caller-provided command, shell fragment, URL, script, or write action.

mod davinci;
mod eagle;
mod model;
mod system;

pub use davinci::{DaVinciProvider, DaVinciRuntime, ResolveBridgeResponse, ResolveDiscovery};
pub use eagle::{EagleProvider, EagleReadEndpoint, EagleRuntime, HttpReadResponse};
pub use model::{
    DaVinciFacts, EagleFacts, ProcessState, ProviderFacts, ProviderKind, ProviderReport,
    ProviderStatus, ResolveEnvironmentFacts, ResolveEnvironmentStatus, RuntimeFailure,
};
pub use system::{SystemDaVinciRuntime, SystemEagleRuntime};

/// Probe both supported providers using the production, read-only runtimes.
pub fn probe_all() -> Vec<ProviderReport> {
    vec![
        EagleProvider::new(SystemEagleRuntime).probe(),
        DaVinciProvider::new(SystemDaVinciRuntime).probe(),
    ]
}
