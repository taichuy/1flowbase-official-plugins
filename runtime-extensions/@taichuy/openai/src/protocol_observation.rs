//! Thin local wire-type adapter; collection and transport hooks live in the shared SDK.
provider_observation::provider_adapter!();

#[cfg(test)]
#[path = "_tests/protocol_observation.rs"]
mod tests;
