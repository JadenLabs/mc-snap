//! mc-snap: declarative Minecraft server management.
//!
//! This crate ships both a binary (`mc-snap`) and a library surface used by
//! integration tests. The module layout mirrors the original multi-crate split:
//! `yml`/`lock`/`cache`/`download`/`paths`/`snapshot`/`proclock`/`state`/`traits`
//! are the core types; `providers` and `loaders` are the pluggable resolvers;
//! `runtime` handles process and Java management; `commands`/`compat`/
//! `orchestrate`/`props` implement the CLI surface.

pub mod cache;
pub mod download;
pub mod lock;
pub mod paths;
pub mod proclock;
pub mod snapshot;
pub mod state;
pub mod traits;
pub mod yml;

pub mod loaders;
pub mod providers;
pub mod runtime;

pub mod commands;
pub mod compat;
pub mod orchestrate;
pub mod props;

pub use traits::{
    AvailableVersion, LaunchCtx, LoaderSpec, ModProvider, ModSpec, ResolveEnv, ResolvedLoader,
    ResolvedMod, ServerLoader,
};
