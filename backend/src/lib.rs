pub mod alarm_mqtt;
pub mod api;
pub mod config;
pub mod control_optimizer;
pub mod models;
pub mod modbus_receiver;
pub mod mqtt;
pub mod parameter_id;
pub mod qlearning;
pub mod rl_control;
pub mod storage;
pub mod thermodynamics;
pub mod thermodynamics_simulator;

pub use alarm_mqtt::{AlarmMqttRequest, AlarmMqttResponse, AlarmMqttService};
pub use api::{build_router, AppState, SharedState, ControlAlgo};
pub use config::*;
pub use control_optimizer::{ControlOptimizer, ControlRequest, ControlResponse, ManualOverride};
pub use models::*;
pub use modbus_receiver::{ModbusReceiver, ValidatedReading};
pub use parameter_id::{IdentifiedParams, MultiFurnaceIdentifier, OnlineParameterIdentifier};
pub use qlearning::{MultiFurnaceQLController, QLearningController, QLearningStatus};
pub use storage::ClickHouseStore;
pub use thermodynamics::ThermodynamicsEngine;
pub use thermodynamics_simulator::{
    get_cached_reading, update_cached_reading, TempFieldResult, ThermodynamicsSimulator,
    ThermoRequest, ThermoResponse,
};
pub use rl_control::MultiFurnaceRLController;
pub use mqtt::{AlarmDetector, AlarmThresholds, MqttConfig, MqttPublisher};

pub mod prelude {
    pub use crate::api::build_router;
    pub use crate::models::*;
    pub use crate::config::SystemConfig;
}
