//! Native Spinel counterpart, isolated from the shared toolchain registry.

#[path = "support/emit_and_run.rs"]
mod emit_and_run;
#[path = "support/index_with.rs"]
mod index_with;
#[path = "spinel_toolchain/index_with.rs"]
mod index_with_runtime;
