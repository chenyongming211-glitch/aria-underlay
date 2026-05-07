pub mod adapter_client;
pub mod api;
pub mod authz;
pub mod device;
pub mod engine;
pub mod error;
pub mod ha;
pub mod intent;
pub mod model;
pub mod ops_cli;
pub mod planner;
pub mod proto;
pub mod state;
pub mod telemetry;
pub mod tx;
pub mod utils;
pub mod worker;

pub use error::{AdapterErrorDetail, UnderlayError, UnderlayResult};
