use std::collections::HashMap;
use std::time::Duration;

use serde::{Deserialize, Serialize};

use crate::models::{FurnaceConfig, FurnaceType, ThermoParams};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SystemConfig {
    pub server: ServerConfig,
    pub clickhouse: ClickHouseConfig,
    pub mqtt: MqttConfigSection,
    pub furnaces: Vec<FurnaceConfigEntry>,
    pub channels: ChannelConfig,
    pub control: ControlConfig,
    pub thermodynamics: ThermodynamicsConfig,
    pub alarms: AlarmConfig,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ServerConfig {
    pub host: String,
    pub port: u16,
    pub log_level: String,
}

impl Default for ServerConfig {
    fn default() -> Self {
        Self {
            host: "0.0.0.0".to_string(),
            port: 8080,
            log_level: "info".to_string(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ClickHouseConfig {
    pub url: String,
    pub database: String,
    pub username: String,
    pub password: String,
    pub batch_size: usize,
    pub flush_interval_ms: u64,
    pub skip_check: bool,
    pub auto_init: bool,
}

impl Default for ClickHouseConfig {
    fn default() -> Self {
        Self {
            url: "http://127.0.0.1:8123".to_string(),
            database: "metallurgy_simulation".to_string(),
            username: "default".to_string(),
            password: "".to_string(),
            batch_size: 500,
            flush_interval_ms: 1000,
            skip_check: false,
            auto_init: true,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MqttConfigSection {
    pub broker: String,
    pub port: u16,
    pub username: Option<String>,
    pub password: Option<String>,
    pub topic_prefix: String,
    pub keep_alive_secs: u64,
    pub publish_retries: u32,
    pub outbox_retry_interval_secs: u64,
}

impl Default for MqttConfigSection {
    fn default() -> Self {
        Self {
            broker: "127.0.0.1".to_string(),
            port: 1883,
            username: None,
            password: None,
            topic_prefix: "metallurgy/alarms".to_string(),
            keep_alive_secs: 60,
            publish_retries: 3,
            outbox_retry_interval_secs: 5,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FurnaceConfigEntry {
    pub id: String,
    pub name: String,
    pub furnace_type: String,
    pub volume_m3: f64,
    pub max_temperature: f64,
    pub target_temp_min: f64,
    pub target_temp_max: f64,
    pub thermodynamics: ThermoParamsEntry,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ThermoParamsEntry {
    pub heat_conductivity: f64,
    pub specific_heat: f64,
    pub reaction_enthalpy: f64,
    pub activation_energy: f64,
    pub pre_exponential_factor: f64,
    pub heat_loss_coefficient: f64,
    pub air_preheat_temp: f64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChannelConfig {
    pub sensor_rx_buffer: usize,
    pub thermo_rx_buffer: usize,
    pub control_rx_buffer: usize,
    pub alarm_rx_buffer: usize,
    pub action_broadcast: usize,
    pub storage_buffer: usize,
}

impl Default for ChannelConfig {
    fn default() -> Self {
        Self {
            sensor_rx_buffer: 2000,
            thermo_rx_buffer: 512,
            control_rx_buffer: 512,
            alarm_rx_buffer: 512,
            action_broadcast: 2000,
            storage_buffer: 4096,
        }
    }
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ControlAlgorithm {
    QLearning,
    Ddpg,
}

impl Default for ControlAlgorithm {
    fn default() -> Self {
        ControlAlgorithm::QLearning
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ControlConfig {
    pub default_algo: ControlAlgorithm,
    pub train_interval_steps: usize,
    pub epsilon_start: f64,
    pub epsilon_min: f64,
    pub epsilon_decay: f64,
    pub learning_rate: f64,
    pub gamma: f64,
    pub q_table_bins: QTableBinsConfig,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct QTableBinsConfig {
    pub temperature: usize,
    pub co: usize,
    pub efficiency: usize,
    pub frequency: usize,
    pub stroke: usize,
}

impl Default for ControlConfig {
    fn default() -> Self {
        Self {
            default_algo: ControlAlgorithm::QLearning,
            train_interval_steps: 2,
            epsilon_start: 0.8,
            epsilon_min: 0.05,
            epsilon_decay: 0.9995,
            learning_rate: 0.08,
            gamma: 0.92,
            q_table_bins: QTableBinsConfig {
                temperature: 8,
                co: 4,
                efficiency: 3,
                frequency: 5,
                stroke: 5,
            },
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ThermodynamicsConfig {
    pub prediction_enabled: bool,
    pub predict_dt_secs: f64,
    pub temp_field_resolution: usize,
    pub param_id_enabled: bool,
    pub param_id_interval: usize,
    pub param_id_min_samples: usize,
    pub param_id_confidence_threshold: f64,
}

impl Default for ThermodynamicsConfig {
    fn default() -> Self {
        Self {
            prediction_enabled: true,
            predict_dt_secs: 10.0,
            temp_field_resolution: 64,
            param_id_enabled: true,
            param_id_interval: 3,
            param_id_min_samples: 10,
            param_id_confidence_threshold: 0.35,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AlarmConfig {
    pub cooldown_secs: HashMap<String, u64>,
    pub temp_too_high_critical: f64,
    pub temp_too_high_warning: f64,
    pub temp_too_low_warning: f64,
    pub temp_too_low_critical: f64,
    pub co_accumulation_warning: f64,
    pub co_accumulation_critical: f64,
    pub pressure_abnormal_warning: f64,
    pub efficiency_low_warning: f64,
}

impl Default for AlarmConfig {
    fn default() -> Self {
        let mut cooldown = HashMap::new();
        cooldown.insert("default".to_string(), 60);
        Self {
            cooldown_secs: cooldown,
            temp_too_high_critical: 1600.0,
            temp_too_high_warning: 1500.0,
            temp_too_low_warning: 800.0,
            temp_too_low_critical: 500.0,
            co_accumulation_warning: 500.0,
            co_accumulation_critical: 800.0,
            pressure_abnormal_warning: 2500.0,
            efficiency_low_warning: 0.35,
        }
    }
}

impl SystemConfig {
    pub fn default_furnaces() -> Vec<FurnaceConfigEntry> {
        vec![
            FurnaceConfigEntry {
                id: "HAN-001".to_string(),
                name: "汉代炒钢炉一号".to_string(),
                furnace_type: "Han_Chaogang".to_string(),
                volume_m3: 2.5,
                max_temperature: 1450.0,
                target_temp_min: 1200.0,
                target_temp_max: 1350.0,
                thermodynamics: ThermoParamsEntry {
                    heat_conductivity: 45.0,
                    specific_heat: 650.0,
                    reaction_enthalpy: -824000.0,
                    activation_energy: 160000.0,
                    pre_exponential_factor: 5.0e8,
                    heat_loss_coefficient: 0.015,
                    air_preheat_temp: 200.0,
                },
            },
            FurnaceConfigEntry {
                id: "MING-001".to_string(),
                name: "明代高炉一号".to_string(),
                furnace_type: "Ming_Blast".to_string(),
                volume_m3: 8.0,
                max_temperature: 1600.0,
                target_temp_min: 1350.0,
                target_temp_max: 1500.0,
                thermodynamics: ThermoParamsEntry {
                    heat_conductivity: 52.0,
                    specific_heat: 700.0,
                    reaction_enthalpy: -850000.0,
                    activation_energy: 165000.0,
                    pre_exponential_factor: 6.5e8,
                    heat_loss_coefficient: 0.012,
                    air_preheat_temp: 300.0,
                },
            },
        ]
    }

    pub fn furnace_configs(&self) -> Vec<(FurnaceConfig, ThermoParams)> {
        self.furnaces
            .iter()
            .map(|f| {
                let furnace_type = FurnaceType::from_str(&f.furnace_type)
                    .unwrap_or(FurnaceType::HanChaogang);
                let fc = FurnaceConfig {
                    furnace_id: f.id.clone(),
                    furnace_name: f.name.clone(),
                    furnace_type,
                    volume_m3: f.volume_m3,
                    max_temperature: f.max_temperature,
                    target_temp_min: f.target_temp_min,
                    target_temp_max: f.target_temp_max,
                };
                let tp = ThermoParams {
                    furnace_id: f.id.clone(),
                    heat_conductivity: f.thermodynamics.heat_conductivity,
                    specific_heat: f.thermodynamics.specific_heat,
                    reaction_enthalpy: f.thermodynamics.reaction_enthalpy,
                    activation_energy: f.thermodynamics.activation_energy,
                    pre_exponential_factor: f.thermodynamics.pre_exponential_factor,
                    heat_loss_coefficient: f.thermodynamics.heat_loss_coefficient,
                    air_preheat_temp: f.thermodynamics.air_preheat_temp,
                };
                (fc, tp)
            })
            .collect()
    }

    pub fn from_env() -> Self {
        use std::env;
        let mut cfg = Self::default();

        if let Ok(h) = env::var("SERVER_HOST") {
            cfg.server.host = h;
        }
        if let Ok(p) = env::var("SERVER_PORT") {
            if let Ok(v) = p.parse::<u16>() {
                cfg.server.port = v;
            }
        }
        if let Ok(l) = env::var("LOG_LEVEL") {
            cfg.server.log_level = l;
        }

        if let Ok(u) = env::var("CLICKHOUSE_URL") {
            cfg.clickhouse.url = u;
        }
        if let Ok(d) = env::var("CLICKHOUSE_DB") {
            cfg.clickhouse.database = d;
        }
        if let Ok(u) = env::var("CLICKHOUSE_USER") {
            cfg.clickhouse.username = u;
        }
        if let Ok(p) = env::var("CLICKHOUSE_PASSWORD") {
            cfg.clickhouse.password = p;
        }

        if let Ok(b) = env::var("MQTT_BROKER") {
            cfg.mqtt.broker = b;
        }
        if let Ok(p) = env::var("MQTT_PORT") {
            if let Ok(v) = p.parse::<u16>() {
                cfg.mqtt.port = v;
            }
        }
        if let Ok(u) = env::var("MQTT_USERNAME") {
            cfg.mqtt.username = Some(u);
        }
        if let Ok(p) = env::var("MQTT_PASSWORD") {
            cfg.mqtt.password = Some(p);
        }
        if let Ok(t) = env::var("MQTT_TOPIC_PREFIX") {
            cfg.mqtt.topic_prefix = t;
        }

        if let Ok(a) = env::var("CONTROL_ALGO") {
            cfg.control.default_algo = match a.to_lowercase().as_str() {
                "ddpg" => ControlAlgorithm::Ddpg,
                _ => ControlAlgorithm::QLearning,
            };
        }

        cfg
    }
}

impl Default for SystemConfig {
    fn default() -> Self {
        Self {
            server: ServerConfig::default(),
            clickhouse: ClickHouseConfig::default(),
            mqtt: MqttConfigSection::default(),
            furnaces: Self::default_furnaces(),
            channels: ChannelConfig::default(),
            control: ControlConfig::default(),
            thermodynamics: ThermodynamicsConfig::default(),
            alarms: AlarmConfig::default(),
        }
    }
}

impl From<&AlarmConfig> for crate::mqtt::AlarmThresholds {
    fn from(cfg: &AlarmConfig) -> Self {
        Self {
            temp_high_warning: cfg.temp_too_high_warning,
            temp_high_critical: cfg.temp_too_high_critical,
            temp_low_warning: cfg.temp_too_low_warning,
            temp_low_critical: cfg.temp_too_low_critical,
            co_warning: cfg.co_accumulation_warning,
            co_critical: cfg.co_accumulation_critical,
            pressure_warning: cfg.pressure_abnormal_warning,
            efficiency_warning: cfg.efficiency_low_warning,
            default_cooldown: Duration::from_secs(
                *cfg.cooldown_secs.get("default").unwrap_or(&60),
            ),
        }
    }
}
