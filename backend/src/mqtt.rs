use std::sync::Arc;
use std::time::Duration;

use anyhow::{Context, Result};
use rumqttc::{
    AsyncClient, MqttOptions, QoS, Event, Packet};
use serde::{Deserialize, Serialize};
use tokio::sync::broadcast;
use tracing::{debug, error, info, warn};

use crate::models::{AlarmEvent, AlarmLevel, AlarmType};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MqttConfig {
    pub broker_url: String,
    pub port: u16,
    pub client_id: String,
    pub username: Option<String>,
    pub password: Option<String>,
    pub topic_prefix: String,
    pub keep_alive: u64,
}

impl Default for MqttConfig {
    fn default() -> Self {
        Self {
            broker_url: "127.0.0.1".to_string(),
            port: 1883,
            client_id: "metallurgy_simulation_backend".to_string(),
            username: None,
            password: None,
            topic_prefix: "metallurgy/alarms".to_string(),
            keep_alive: 60,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MqttAlarmMessage {
    pub event_id: String,
    pub timestamp: String,
    pub furnace_id: String,
    pub alarm_type: String,
    pub alarm_level: String,
    pub message: String,
    pub current_value: f64,
    pub threshold_value: f64,
    pub description: String,
}

impl From<&AlarmEvent> for MqttAlarmMessage {
    fn from(a: &AlarmEvent) -> Self {
        let level_desc = match a.alarm_level {
            AlarmLevel::Warning => "注意",
            AlarmLevel::Critical => "严重",
            AlarmLevel::Fatal => "致命",
        };
        let type_desc = match a.alarm_type {
            AlarmType::TempTooHigh => "炉温过高",
            AlarmType::TempTooLow => "炉温过低",
            AlarmType::CoAccumulation => "CO积聚",
            AlarmType::PressureAbnormal => "风压异常",
            AlarmType::EfficiencyLow => "效率过低",
            AlarmType::SystemError => "系统错误",
        };

        Self {
            event_id: a.event_id.to_string(),
            timestamp: a.timestamp.to_rfc3339(),
            furnace_id: a.furnace_id.clone(),
            alarm_type: a.alarm_type.as_str().to_string(),
            alarm_level: a.alarm_level.as_str().to_string(),
            message: a.message.clone(),
            current_value: a.current_value,
            threshold_value: a.threshold_value,
            description: format!("[{}][{}] {} | 当前:{:.2}, 阈值:{:.2}",
                level_desc, type_desc, a.message, a.current_value, a.threshold_value),
        }
    }
}

pub struct AlarmDetector {
    thresholds: std::collections::HashMap<String, AlarmThresholds>,
    cooldown_map: std::collections::HashMap<(String, AlarmType), chrono::DateTime<chrono::Utc>>,
    default_cooldown: chrono::Duration,
}

#[derive(Debug, Clone)]
pub struct AlarmThresholds {
    pub temp_max: f64,
    pub temp_min: f64,
    pub temp_target_max: f64,
    pub temp_target_min: f64,
    pub co_warning: f64,
    pub co_critical: f64,
    pub pressure_min: f64,
    pub pressure_max: f64,
    pub efficiency_min: f64,
}

impl Default for AlarmThresholds {
    pub fn for_han_chaogang() -> Self {
        Self {
            temp_max: 1450.0,
            temp_min: 600.0,
            temp_target_max: 1350.0,
            temp_target_min: 1200.0,
            co_warning: 2.0,
            co_critical: 4.0,
            pressure_min: 200.0,
            pressure_max: 3000.0,
            efficiency_min: 25.0,
        }
    }

    pub fn for_ming_blast() -> Self {
        Self {
            temp_max: 1600.0,
            temp_min: 700.0,
            temp_target_max: 1500.0,
            temp_target_min: 1350.0,
            co_warning: 2.5,
            co_critical: 5.0,
            pressure_min: 400.0,
            pressure_max: 4500.0,
            efficiency_min: 30.0,
        }
    }
}

impl AlarmDetector {
    pub fn new() -> Self {
        let mut thresholds = std::collections::HashMap::new();
        thresholds.insert("HAN-001".to_string(), AlarmThresholds::for_han_chaogang());
        thresholds.insert("MING-001".to_string(), AlarmThresholds::for_ming_blast());

        Self {
            thresholds,
            cooldown_map: std::collections::HashMap::new(),
            default_cooldown: chrono::Duration::seconds(60),
        }
    }

    pub fn set_thresholds(&mut self, furnace_id: String, thresholds: AlarmThresholds) {
        self.thresholds.insert(furnace_id, thresholds);
    }

    fn check_cooldown(&mut self, furnace_id: &str, alarm_type: AlarmType) -> bool {
        let now = chrono::Utc::now();
        let key = (furnace_id.to_string(), alarm_type);

        if let Some(last) = self.cooldown_map.get(&key) {
            if now - *last < self.default_cooldown {
                return false;
            }
        }
        self.cooldown_map.insert(key, now);
        true
    }

    pub fn detect_alarms(
        &mut self,
        reading: &crate::models::SensorReading,
    ) -> Vec<AlarmEvent> {
        let mut alarms = Vec::new();
        let thresholds = match self.thresholds.get(&reading.furnace_id) {
            Some(t) => t.clone(),
            None => {
                warn!("未找到炉 {} 的告警阈值，使用默认", reading.furnace_id);
                AlarmThresholds::for_han_chaogang()
            }
        };

        alarms
    }

    pub fn detect_from_reading(
        &mut self,
        reading: &crate::models::SensorReading,
    ) -> Vec<AlarmEvent> {
        let mut alarms: Vec<AlarmEvent> = Vec::new();
        let now = chrono::Utc::now();

        let thresholds = self.thresholds.get(&reading.furnace_id).cloned().unwrap_or_else(|| {
            if reading.furnace_id.starts_with("MING") {
                AlarmThresholds::for_ming_blast()
            } else {
                AlarmThresholds::for_han_chaogang()
            }
        });

        let temp = reading.furnace_temp;
        let co = reading.co_concentration;
        let pressure = reading.wind_pressure;
        let efficiency = reading.energy_efficiency;
        let fid = &reading.furnace_id;

        if temp > thresholds.temp_max {
            if self.check_cooldown(fid, AlarmType::TempTooHigh) {
                alarms.push(AlarmEvent {
                    event_id: uuid::Uuid::new_v4(),
                    timestamp: now,
                    furnace_id: fid.clone(),
                    alarm_type: AlarmType::TempTooHigh,
                    alarm_level: AlarmLevel::Critical,
                    message: format!("炉温超过极限温度 {:.1}°C，可能导致炉体损伤", thresholds.temp_max),
                    current_value: temp,
                    threshold_value: thresholds.temp_max,
                    acknowledged: 0,
                    mqtt_published: 0,
                });
            }
        } else if temp > thresholds.temp_target_max + 100.0 {
            if self.check_cooldown(fid, AlarmType::TempTooHigh) {
                alarms.push(AlarmEvent {
                    event_id: uuid::Uuid::new_v4(),
                    timestamp: now,
                    furnace_id: fid.clone(),
                    alarm_type: AlarmType::TempTooHigh,
                    alarm_level: AlarmLevel::Warning,
                    message: format!("炉温高于目标区间上限，建议加强散热"),
                    current_value: temp,
                    threshold_value: thresholds.temp_target_max,
                    acknowledged: 0,
                    mqtt_published: 0,
                });
            }
        }

        if temp < thresholds.temp_min {
            if self.check_cooldown(fid, AlarmType::TempTooLow) {
                alarms.push(AlarmEvent {
                    event_id: uuid::Uuid::new_v4(),
                    timestamp: now,
                    furnace_id: fid.clone(),
                    alarm_type: AlarmType::TempTooLow,
                    alarm_level: AlarmLevel::Critical,
                    message: format!("炉温低于安全下限 {:.1}°C，反应无法进行", thresholds.temp_min),
                    current_value: temp,
                    threshold_value: thresholds.temp_min,
                    acknowledged: 0,
                    mqtt_published: 0,
                });
            }
        } else if temp < thresholds.temp_target_min - 80.0 {
            if self.check_cooldown(fid, AlarmType::TempTooLow) {
                alarms.push(AlarmEvent {
                    event_id: uuid::Uuid::new_v4(),
                    timestamp: now,
                    furnace_id: fid.clone(),
                    alarm_type: AlarmType::TempTooLow,
                    alarm_level: AlarmLevel::Warning,
                    message: format!("炉温低于目标区间，建议加大鼓风"),
                    current_value: temp,
                    threshold_value: thresholds.temp_target_min,
                    acknowledged: 0,
                    mqtt_published: 0,
                });
            }
        }

        if co > thresholds.co_critical {
            if self.check_cooldown(fid, AlarmType::CoAccumulation) {
                alarms.push(AlarmEvent {
                    event_id: uuid::Uuid::new_v4(),
                    timestamp: now,
                    furnace_id: fid.clone(),
                    alarm_type: AlarmType::CoAccumulation,
                    alarm_level: AlarmLevel::Fatal,
                    message: format!("CO浓度严重超标！有爆炸和中毒危险，立即加强通风"),
                    current_value: co,
                    threshold_value: thresholds.co_critical,
                    acknowledged: 0,
                    mqtt_published: 0,
                });
            }
        } else if co > thresholds.co_warning {
            if self.check_cooldown(fid, AlarmType::CoAccumulation) {
                alarms.push(AlarmEvent {
                    event_id: uuid::Uuid::new_v4(),
                    timestamp: now,
                    furnace_id: fid.clone(),
                    alarm_type: AlarmType::CoAccumulation,
                    alarm_level: AlarmLevel::Warning,
                    message: format!("CO浓度偏高，请注意通风"),
                    current_value: co,
                    threshold_value: thresholds.co_warning,
                    acknowledged: 0,
                    mqtt_published: 0,
                });
            }
        }

        if pressure < thresholds.pressure_min || pressure > thresholds.pressure_max {
            if self.check_cooldown(fid, AlarmType::PressureAbnormal) {
                let level = if pressure < thresholds.pressure_min { "过低" } else { "过高" };
                alarms.push(AlarmEvent {
                    event_id: uuid::Uuid::new_v4(),
                    timestamp: now,
                    furnace_id: fid.clone(),
                    alarm_type: AlarmType::PressureAbnormal,
                    alarm_level: AlarmLevel::Warning,
                    message: format!("风压{}，请检查风箱运行状态"),
                    current_value: pressure,
                    threshold_value: if pressure < thresholds.pressure_min { thresholds.pressure_min } else { thresholds.pressure_max },
                    acknowledged: 0,
                    mqtt_published: 0,
                });
            }
        }

        if efficiency > 0.0 && efficiency < thresholds.efficiency_min {
            if self.check_cooldown(fid, AlarmType::EfficiencyLow) {
                alarms.push(AlarmEvent {
                    event_id: uuid::Uuid::new_v4(),
                    timestamp: now,
                    furnace_id: fid.clone(),
                    alarm_type: AlarmType::EfficiencyLow,
                    alarm_level: AlarmLevel::Warning,
                    message: format!("能源效率低于{}%，建议优化操作参数"),
                    current_value: efficiency,
                    threshold_value: thresholds.efficiency_min,
                    acknowledged: 0,
                    mqtt_published: 0,
                });
            }
        }

        alarms
    }
}

impl Default for AlarmDetector {
    fn default() -> Self {
        Self::new()
    }
}

pub struct MqttPublisher {
    client: Option<AsyncClient>,
    config: MqttConfig,
    broadcast_tx: broadcast::Sender<AlarmEvent>,
}

impl MqttPublisher {
    pub fn new(config: MqttConfig) -> Self {
        let (tx, _) = broadcast::channel(1000);
        Self {
            client: None,
            config,
            broadcast_tx: tx,
        }
    }

    pub async fn connect(&mut self) -> Result<()> {
        let mut opts = MqttOptions::new(
            &self.config.client_id.clone(),
            &self.config.broker_url.clone(),
            self.config.port,
        );

        opts.set_keep_alive(Duration::from_secs(self.config.keep_alive));

        if let Some(username) = &self.config.username {
            opts.set_credentials(username, self.config.password.clone().unwrap_or_default());
        }

        let (client, mut eventloop) = AsyncClient::new(opts, 200);
        self.client = Some(client.clone());

        let config = self.config.clone();
        tokio::spawn(async move {
            info!("MQTT Publisher 连接到 broker: {}:{}", config.broker_url, config.port);
            loop {
                match eventloop.poll().await {
                    Ok(Event::Incoming(Packet::ConnAck(_))) => {
                        info!("MQTT Publisher 连接成功");
                    }
                    Ok(Event::Incoming(Packet::Publish(_))) => {}
                    Ok(_) => {}
                    Err(e) => {
                        error!("MQTT eventloop 错误: {}", e);
                        tokio::time::sleep(Duration::from_secs(5)).await;
                    }
                }
            }
        });

        Ok(())
    }

    pub fn subscribe(&self) -> broadcast::Receiver<AlarmEvent> {
        self.broadcast_tx.subscribe()
    }

    pub async fn publish_alarm(&self, alarm: &AlarmEvent) -> Result<()> {
        let msg = MqttAlarmMessage::from(alarm);
        let topic = format!(
            "{}/{}/{}",
            self.config.topic_prefix,
            alarm.furnace_id,
            alarm.alarm_type.as_str().to_lowercase()
        );
        let payload = serde_json::to_vec(&msg)?;

        if let Some(client) = &self.client {
            match client
                .publish(&topic, QoS::AtLeastOnce, false, payload.clone())
                .await
            {
                Ok(_) => {
                    debug!("MQTT告警发布成功: topic={}", topic);
                    let _ = self.broadcast_tx.send(alarm.clone());
                    Ok(())
                }
                Err(e) => {
                    error!("MQTT告警发布失败: {}", e);
                    anyhow::bail!("MQTT publish failed: {}", e);
                }
            }
        } else {
            warn!("MQTT 客户端未初始化，跳过发布，但通过广播发送");
            let _ = self.broadcast_tx.send(alarm.clone());
            Ok(())
        }
    }

    pub async fn publish_with_retry(&self, alarm: &AlarmEvent, max_retries: u32) -> Result<()> {
        for attempt in 0..max_retries {
            match self.publish_alarm(alarm).await {
                Ok(_) => return Ok(()),
                Err(e) => {
                    warn!("MQTT 发布告警失败 (尝试 {}/{}): {}",
                        attempt + 1, max_retries, e);
                    if attempt < max_retries - 1 {
                        tokio::time::sleep(Duration::from_millis(500 * (attempt + 1) as u64)).await;
                    }
                }
            }
        }
        anyhow::bail!("MQTT 发布告警重试超过最大次数")
    }

    pub fn config(&self) -> &MqttConfig {
        &self.config
    }
}

pub struct MockMqttPublisher;

impl MockMqttPublisher {
    pub fn publish_alarm_sync(alarm: &AlarmEvent) -> String {
        let msg = MqttAlarmMessage::from(alarm);
        serde_json::to_string_pretty(&msg).unwrap_or_default()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::models::SensorReading;
    use chrono::Utc;

    fn make_reading(furnace_id: &str, temp: f64, co: f64, pressure: f64, efficiency: f64) -> SensorReading {
        SensorReading {
            timestamp: Utc::now(),
            furnace_id: furnace_id.to_string(),
            push_pull_frequency: 30.0,
            stroke_length: 40.0,
            wind_pressure: pressure,
            air_volume: 0.5,
            furnace_temp: temp,
            co_concentration: co,
            o2_concentration: 18.0,
            iron_feed_rate: 1.0,
            coal_feed_rate: 0.6,
            pig_iron_output: 100.0,
            temp_zone_top: temp - 150.0,
            temp_zone_upper: temp - 80.0,
            temp_zone_middle: temp - 30.0,
            temp_zone_lower: temp + 20.0,
            temp_zone_hearth: temp + 50.0,
            reaction_rate: 0.5,
            energy_efficiency: efficiency,
            quality: 98.0,
            protocol: "Modbus_RTU".to_string(),
            phase: None,
            modbus_frame_hex: None,
        }
    }

    #[test]
    fn test_alarm_detection_temp_too_high() {
        let mut detector = AlarmDetector::new();
        let reading = make_reading("HAN-001", 1500.0, 1.0, 1000.0, 50.0);
        let alarms = detector.detect_from_reading(&reading);
        assert!(!alarms.is_empty(), "应检测到炉温过高告警");
        assert!(alarms.iter().any(|a| matches!(a.alarm_type, AlarmType::TempTooHigh)));
    }

    #[test]
    fn test_alarm_detection_co_accumulation() {
        let mut detector = AlarmDetector::new();
        let reading = make_reading("MING-001", 1400.0, 5.5, 2000.0, 50.0);
        let alarms = detector.detect_from_reading(&reading);
        assert!(alarms.iter().any(|a| matches!(a.alarm_type, AlarmType::CoAccumulation)));
    }
}
