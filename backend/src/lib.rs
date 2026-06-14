pub mod api;
pub mod models;
pub mod mqtt;
pub mod rl_control;
pub mod storage;
pub mod thermodynamics;

pub use api::{build_router, AppState, SharedState};
pub use models::*;
pub use storage::ClickHouseStore;
pub use thermodynamics::ThermodynamicsEngine;
pub use rl_control::MultiFurnaceRLController;
pub use mqtt::{AlarmDetector, AlarmThresholds, MqttConfig, MqttPublisher};

pub mod prelude {
    pub use crate::api::build_router;
    pub use crate::models::*;
}
