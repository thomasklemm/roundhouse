//! End-to-end CRuby test isolated from the shared emit-and-run registry.

#[path = "support/emit_and_run.rs"]
mod emit_and_run;
#[path = "support/index_with.rs"]
mod index_with;
#[path = "emit_and_run/index_with.rs"]
mod index_with_runtime;
