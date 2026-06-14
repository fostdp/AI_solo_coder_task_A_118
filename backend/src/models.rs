use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Hash)]
#[serde(rename_all = "snake_case")]
pub enum FurnaceType {
    HanChaogang,
    MingBlast,
}

impl FurnaceType {
    pub fn as_str(&self) -> &'static str {
        match self {
            FurnaceType::HanChaogang => "Han_Chaogang",
            FurnaceType::MingBlast => "Ming_Blast",
        }
    }

    pub fn from_str(s: &str) -> Option<Self> {
        match s {
            "Han_Chaogang" => Some(FurnaceType::HanChaogang),
            "Ming_Blast" => Some(FurnaceType::MingBlast),
            _ => None,
        }
    }
}

impl Default for FurnaceType {
    fn default() -> Self {
        FurnaceType::HanChaogang
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FurnaceConfig {
    pub furnace_id: String,
    pub furnace_name: String,
    pub furnace_type: FurnaceType,
    pub volume_m3: f64,
    pub max_temperature: f64,
    pub target_temp_min: f64,
    pub target_temp_max: f64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SensorReading {
    #[serde(default = "Utc::now")]
    pub timestamp: DateTime<Utc>,
    pub furnace_id: String,
    pub push_pull_frequency: f64,
    pub stroke_length: f64,
    pub wind_pressure: f64,
    pub air_volume: f64,
    pub furnace_temp: f64,
    pub co_concentration: f64,
    pub o2_concentration: f64,
    pub iron_feed_rate: f64,
    pub coal_feed_rate: f64,
    pub pig_iron_output: f64,
    pub temp_zone_top: f64,
    pub temp_zone_upper: f64,
    pub temp_zone_middle: f64,
    pub temp_zone_lower: f64,
    pub temp_zone_hearth: f64,
    pub reaction_rate: f64,
    pub energy_efficiency: f64,
    #[serde(default = "default_quality")]
    pub quality: f64,
    #[serde(default)]
    pub protocol: String,
    #[serde(default)]
    pub phase: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub modbus_frame_hex: Option<String>,
}

fn default_quality() -> f64 {
    100.0
}

impl SensorReading {
    pub fn temp_zones(&self) -> [f64; 5] {
        [
            self.temp_zone_top,
            self.temp_zone_upper,
            self.temp_zone_middle,
            self.temp_zone_lower,
            self.temp_zone_hearth,
        ]
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ThermoParams {
    pub furnace_id: String,
    pub heat_conductivity: f64,
    pub specific_heat: f64,
    pub reaction_enthalpy: f64,
    pub activation_energy: f64,
    pub pre_exponential_factor: f64,
    pub heat_loss_coefficient: f64,
    pub air_preheat_temp: f64,
}

impl Default for ThermoParams {
    fn default() -> Self {
        Self {
            furnace_id: String::new(),
            heat_conductivity: 45.0,
            specific_heat: 650.0,
            reaction_enthalpy: -824000.0,
            activation_energy: 160000.0,
            pre_exponential_factor: 5.0e8,
            heat_loss_coefficient: 0.015,
            air_preheat_temp: 200.0,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RLState {
    pub furnace_temp: f64,
    pub temp_deviation: f64,
    pub co_concentration: f64,
    pub wind_pressure: f64,
    pub air_volume: f64,
    pub energy_efficiency: f64,
    pub current_frequency: f64,
    pub current_stroke: f64,
    pub reaction_rate: f64,
    pub temp_gradient: f64,
}

impl RLState {
    pub fn to_vector(&self) -> Vec<f64> {
        vec![
            self.furnace_temp,
            self.temp_deviation,
            self.co_concentration,
            self.wind_pressure,
            self.air_volume,
            self.energy_efficiency,
            self.current_frequency,
            self.current_stroke,
            self.reaction_rate,
            self.temp_gradient,
        ]
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RLAction {
    pub frequency: f64,
    pub stroke: f64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ControlStep {
    pub timestamp: DateTime<Utc>,
    pub furnace_id: String,
    pub episode: u32,
    pub step: u32,
    pub state_vector: Vec<f64>,
    pub action_frequency: f64,
    pub action_stroke: f64,
    pub reward: f64,
    pub next_state_vector: Vec<f64>,
    pub done: u8,
    pub loss: f64,
    pub epsilon: f64,
    pub learning_rate: f64,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum AlarmType {
    TempTooHigh,
    TempTooLow,
    CoAccumulation,
    PressureAbnormal,
    EfficiencyLow,
    SystemError,
}

impl AlarmType {
    pub fn as_str(&self) -> &'static str {
        match self {
            AlarmType::TempTooHigh => "TEMP_TOO_HIGH",
            AlarmType::TempTooLow => "TEMP_TOO_LOW",
            AlarmType::CoAccumulation => "CO_ACCUMULATION",
            AlarmType::PressureAbnormal => "PRESSURE_ABNORMAL",
            AlarmType::EfficiencyLow => "EFFICIENCY_LOW",
            AlarmType::SystemError => "SYSTEM_ERROR",
        }
    }
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum AlarmLevel {
    Warning,
    Critical,
    Fatal,
}

impl AlarmLevel {
    pub fn as_str(&self) -> &'static str {
        match self {
            AlarmLevel::Warning => "WARNING",
            AlarmLevel::Critical => "CRITICAL",
            AlarmLevel::Fatal => "FATAL",
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AlarmEvent {
    #[serde(default = "Uuid::new_v4")]
    pub event_id: Uuid,
    pub timestamp: DateTime<Utc>,
    pub furnace_id: String,
    pub alarm_type: AlarmType,
    pub alarm_level: AlarmLevel,
    pub message: String,
    pub current_value: f64,
    pub threshold_value: f64,
    #[serde(default)]
    pub acknowledged: u8,
    #[serde(default)]
    pub mqtt_published: u8,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ThermoPrediction {
    pub timestamp: DateTime<Utc>,
    pub furnace_id: String,
    pub predicted_temp: f64,
    pub predicted_co: f64,
    pub predicted_reaction_rate: f64,
    pub predicted_efficiency: f64,
    pub temp_distribution: Vec<f64>,
    pub iron_output_rate: f64,
    pub confidence: f64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ApiResponse<T> {
    pub success: bool,
    pub data: Option<T>,
    pub message: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub recommended_action: Option<RLAction>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub alarms: Option<Vec<AlarmEvent>>,
}

impl<T> ApiResponse<T> {
    pub fn ok(data: T) -> Self {
        Self {
            success: true,
            data: Some(data),
            message: None,
            recommended_action: None,
            alarms: None,
        }
    }

    pub fn ok_with_action(data: T, action: RLAction) -> Self {
        Self {
            success: true,
            data: Some(data),
            message: None,
            recommended_action: Some(action),
            alarms: None,
        }
    }

    pub fn error(msg: &str) -> Self {
        Self {
            success: false,
            data: None,
            message: Some(msg.to_string()),
            recommended_action: None,
            alarms: None,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProductionStats {
    pub stat_date: String,
    pub furnace_id: String,
    pub total_iron_kg: f64,
    pub total_coal_kg: f64,
    pub total_iron_ore_kg: f64,
    pub avg_temp: f64,
    pub avg_co_concentration: f64,
    pub avg_energy_efficiency: f64,
    pub operation_hours: f64,
    pub alarm_count: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WSMessage {
    pub msg_type: String,
    pub furnace_id: Option<String>,
    pub data: serde_json::Value,
    pub timestamp: DateTime<Utc>,
}

impl WSMessage {
    pub fn sensor(reading: &SensorReading) -> Self {
        Self {
            msg_type: "sensor_data".to_string(),
            furnace_id: Some(reading.furnace_id.clone()),
            data: serde_json::to_value(reading).unwrap_or_default(),
            timestamp: Utc::now(),
        }
    }

    pub fn alarm(alarm: &AlarmEvent) -> Self {
        Self {
            msg_type: "alarm".to_string(),
            furnace_id: Some(alarm.furnace_id.clone()),
            data: serde_json::to_value(alarm).unwrap_or_default(),
            timestamp: Utc::now(),
        }
    }

    pub fn action(furnace_id: &str, action: &RLAction) -> Self {
        Self {
            msg_type: "control_action".to_string(),
            furnace_id: Some(furnace_id.to_string()),
            data: serde_json::to_value(action).unwrap_or_default(),
            timestamp: Utc::now(),
        }
    }
}
