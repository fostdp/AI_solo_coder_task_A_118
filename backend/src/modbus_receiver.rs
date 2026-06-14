use std::collections::HashMap;
use std::sync::Arc;
use std::time::Instant;

use chrono::Utc;
use serde::{Deserialize, Serialize};
use tokio::sync::{mpsc, Mutex};
use tracing::{debug, error, info, warn};

use crate::config::SystemConfig;
use crate::models::SensorReading;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ValidatedReading {
    pub reading: SensorReading,
    pub received_at: chrono::DateTime<Utc>,
    pub latency_us: u64,
    pub source: ReadingSource,
    pub modbus_valid: bool,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ReadingSource {
    ModbusRtu,
    ModbusTcp,
    HttpInjection,
    Simulator,
    Replay,
}

impl Default for ReadingSource {
    fn default() -> Self {
        ReadingSource::HttpInjection
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ValidationReport {
    pub received: u64,
    pub accepted: u64,
    pub rejected: u64,
    pub modbus_frames_invalid: u64,
    pub out_of_range: u64,
    pub timestamp_invalid: u64,
    pub last_error: Option<String>,
    pub by_furnace: HashMap<String, FurnaceReceiveStats>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct FurnaceReceiveStats {
    pub received: u64,
    pub accepted: u64,
    pub rejected: u64,
    pub avg_latency_us: f64,
    pub last_reading_at: Option<chrono::DateTime<Utc>>,
}

impl Default for ValidationReport {
    fn default() -> Self {
        Self {
            received: 0,
            accepted: 0,
            rejected: 0,
            modbus_frames_invalid: 0,
            out_of_range: 0,
            timestamp_invalid: 0,
            last_error: None,
            by_furnace: HashMap::new(),
        }
    }
}

const MODBUS_CRC16_POLY: u16 = 0xA001;

fn modbus_crc16(data: &[u8]) -> u16 {
    let mut crc: u16 = 0xFFFF;
    for &b in data {
        crc ^= b as u16;
        for _ in 0..8 {
            if crc & 0x0001 != 0 {
                crc = (crc >> 1) ^ MODBUS_CRC16_POLY;
            } else {
                crc >>= 1;
            }
        }
    }
    crc
}

fn hex_to_bytes(s: &str) -> Option<Vec<u8>> {
    let s = s.trim().replace([' ', '\t', '\n'], "");
    if s.len() % 2 != 0 {
        return None;
    }
    (0..s.len())
        .step_by(2)
        .map(|i| u8::from_str_radix(&s[i..i + 2], 16).ok())
        .collect()
}

#[derive(Debug)]
pub struct ValidationError {
    pub code: &'static str,
    pub field: Option<String>,
    pub message: String,
    pub value: Option<String>,
}

impl std::fmt::Display for ValidationError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "[{}] {:?}: {}", self.code, self.field, self.message)
    }
}

pub struct ModbusReceiver {
    config: Arc<SystemConfig>,
    valid_ranges: HashMap<&'static str, (f64, f64)>,
    stats: Arc<Mutex<ValidationReport>>,
    last_modbus_locks: Arc<Mutex<HashMap<String, chrono::DateTime<Utc>>>>,
    min_interval_secs: f64,
}

impl ModbusReceiver {
    pub fn new(config: Arc<SystemConfig>) -> Self {
        let mut valid_ranges = HashMap::new();
        valid_ranges.insert("push_pull_frequency", (5.0, 120.0));
        valid_ranges.insert("stroke_length", (5.0, 150.0));
        valid_ranges.insert("wind_pressure", (100.0, 10_000.0));
        valid_ranges.insert("air_volume", (0.001, 20.0));
        valid_ranges.insert("furnace_temp", (50.0, 2500.0));
        valid_ranges.insert("co_concentration", (0.0, 5000.0));
        valid_ranges.insert("o2_concentration", (0.0, 25.0));
        valid_ranges.insert("iron_feed_rate", (0.0, 100.0));
        valid_ranges.insert("coal_feed_rate", (0.0, 100.0));
        valid_ranges.insert("pig_iron_output", (0.0, 10_000.0));
        valid_ranges.insert("reaction_rate", (0.0, 1_000_000.0));
        valid_ranges.insert("energy_efficiency", (0.0, 1.0));

        Self {
            config,
            valid_ranges,
            stats: Arc::new(Mutex::new(ValidationReport::default())),
            last_modbus_locks: Arc::new(Mutex::new(HashMap::new())),
            min_interval_secs: 0.5,
        }
    }

    pub async fn start(
        mut self,
        mut sensor_rx: mpsc::Receiver<SensorReading>,
        validated_tx: mpsc::Sender<ValidatedReading>,
        mut raw_modbus_rx: Option<mpsc::Receiver<Vec<u8>>>,
    ) {
        info!("ModbusReceiver 启动，范围校验启用");

        let modbus_worker = if let Some(rx) = raw_modbus_rx.take() {
            let modbus_sender = validated_tx.clone();
            let config = self.config.clone();
            let stats = self.stats.clone();
            let locks = self.last_modbus_locks.clone();
            Some(tokio::spawn(async move {
                Self::modbus_raw_worker(rx, modbus_sender, config, stats, locks).await;
            }))
        } else {
            None
        };

        while let Some(reading) = sensor_rx.recv().await {
            let received_at = Instant::now();

            match self.validate_and_enrich(&reading).await {
                Ok(validated) => {
                    let elapsed = received_at.elapsed();
                    let mut valid = validated.clone();
                    valid.latency_us = elapsed.as_micros() as u64;
                    let furnace_id = reading.furnace_id.clone();
                    let accepted = valid.clone();

                    if let Err(e) = validated_tx.send(valid).await {
                        error!("ValidatedReading 通道发送失败: {}", e);
                    }

                    self.record_stats(furnace_id, accepted, None, elapsed).await;
                }
                Err(e) => {
                    warn!("传感器数据校验失败: {} (furnace={})", e, reading.furnace_id);
                    self.record_stats(
                        reading.furnace_id.clone(),
                        None,
                        Some(e),
                        received_at.elapsed(),
                    )
                    .await;
                }
            }
        }

        if let Some(h) = modbus_worker {
            let _ = h.await;
        }
        info!("ModbusReceiver 退出");
    }

    async fn validate_and_enrich(
        &mut self,
        reading: &SensorReading,
    ) -> Result<ValidatedReading, ValidationError> {
        if reading.furnace_id.is_empty() {
            return Err(ValidationError {
                code: "EMPTY_FURNACE_ID",
                field: Some("furnace_id".into()),
                message: "furnace_id 不能为空".into(),
                value: None,
            });
        }

        let now = Utc::now();
        let ts = reading.timestamp;
        let diff = (now - ts).num_seconds();
        if diff > 86400 || diff < -3600 {
            return Err(ValidationError {
                code: "INVALID_TIMESTAMP",
                field: Some("timestamp".into()),
                message: format!("时间戳偏差过大: {}s", diff),
                value: Some(ts.to_rfc3339()),
            });
        }

        for (field, (min, max)) in &self.valid_ranges {
            let value = match *field {
                "push_pull_frequency" => Some(reading.push_pull_frequency),
                "stroke_length" => Some(reading.stroke_length),
                "wind_pressure" => Some(reading.wind_pressure),
                "air_volume" => Some(reading.air_volume),
                "furnace_temp" => Some(reading.furnace_temp),
                "co_concentration" => Some(reading.co_concentration),
                "o2_concentration" => Some(reading.o2_concentration),
                "iron_feed_rate" => Some(reading.iron_feed_rate),
                "coal_feed_rate" => Some(reading.coal_feed_rate),
                "pig_iron_output" => Some(reading.pig_iron_output),
                "reaction_rate" => Some(reading.reaction_rate),
                "energy_efficiency" => Some(reading.energy_efficiency),
                _ => None,
            };
            if let Some(v) = value {
                if !v.is_finite() || v < *min || v > *max {
                    return Err(ValidationError {
                        code: "OUT_OF_RANGE",
                        field: Some((*field).to_string()),
                        message: format!("超出合理范围 [{}, {}]", min, max),
                        value: Some(v.to_string()),
                    });
                }
            }
        }

        let temp_zones = reading.temp_zones();
        for (i, t) in temp_zones.iter().enumerate() {
            if !t.is_finite() || *t < 0.0 || *t > 3000.0 {
                return Err(ValidationError {
                    code: "OUT_OF_RANGE",
                    field: Some(format!("temp_zone_{}", i)),
                    message: format!("温度区{}非法: {}", i, t),
                    value: Some(t.to_string()),
                });
            }
        }

        let modbus_valid = match &reading.modbus_frame_hex {
            Some(hex) => match hex_to_bytes(hex) {
                Some(bytes) if bytes.len() >= 5 => {
                    let payload_end = bytes.len() - 2;
                    let payload = &bytes[0..payload_end];
                    let expected_crc = modbus_crc16(payload);
                    let actual_crc =
                        (bytes[payload_end + 1] as u16) << 8 | (bytes[payload_end] as u16);
                    expected_crc == actual_crc
                }
                _ => false,
            },
            None => true,
        };

        let source = if reading.protocol.eq_ignore_ascii_case("modbus_rtu") {
            ReadingSource::ModbusRtu
        } else if reading.protocol.eq_ignore_ascii_case("modbus_tcp") {
            ReadingSource::ModbusTcp
        } else if reading.protocol.eq_ignore_ascii_case("simulator") {
            ReadingSource::Simulator
        } else if reading.protocol.eq_ignore_ascii_case("replay") {
            ReadingSource::Replay
        } else {
            ReadingSource::HttpInjection
        };

        Ok(ValidatedReading {
            reading: reading.clone(),
            received_at: now,
            latency_us: 0,
            source,
            modbus_valid,
        })
    }

    async fn record_stats(
        &self,
        furnace_id: String,
        validated: Option<ValidatedReading>,
        error: Option<ValidationError>,
        latency: std::time::Duration,
    ) {
        let mut stats = self.stats.lock().await;
        stats.received += 1;
        if let Some(v) = validated {
            stats.accepted += 1;
            if !v.modbus_valid {
                stats.modbus_frames_invalid += 1;
            }
            let s = stats.by_furnace.entry(furnace_id).or_default();
            s.received += 1;
            s.accepted += 1;
            let n = s.accepted as f64;
            s.avg_latency_us =
                s.avg_latency_us * (n - 1.0) / n + (latency.as_micros() as f64) / n;
            s.last_reading_at = Some(v.received_at);
        } else {
            stats.rejected += 1;
            if let Some(e) = error {
                match e.code {
                    "OUT_OF_RANGE" => stats.out_of_range += 1,
                    "INVALID_TIMESTAMP" => stats.timestamp_invalid += 1,
                    _ => {}
                }
                stats.last_error = Some(e.to_string());
            }
            let s = stats.by_furnace.entry(furnace_id).or_default();
            s.received += 1;
            s.rejected += 1;
        }
    }

    pub async fn get_stats(&self) -> ValidationReport {
        self.stats.lock().await.clone()
    }

    async fn modbus_raw_worker(
        mut rx: mpsc::Receiver<Vec<u8>>,
        _validated_tx: mpsc::Sender<ValidatedReading>,
        _config: Arc<SystemConfig>,
        stats: Arc<Mutex<ValidationReport>>,
        _locks: Arc<Mutex<HashMap<String, chrono::DateTime<Utc>>>>,
    ) {
        debug!("原始Modbus帧worker启动");
        while let Some(_frame) = rx.recv().await {
            let mut s = stats.lock().await;
            s.received += 1;
            s.last_error = Some("Raw Modbus 帧解析占位: 接入真实设备后实现寄存器解码".into());
        }
        debug!("原始Modbus帧worker退出");
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_crc16_basic() {
        let data = [0x01u8, 0x03, 0x00, 0x00, 0x00, 0x0A];
        let crc = modbus_crc16(&data);
        assert_eq!(crc, 0xC5CD);
    }

    #[test]
    fn test_hex_to_bytes() {
        assert_eq!(hex_to_bytes("0103"), Some(vec![0x01, 0x03]));
        assert_eq!(hex_to_bytes("01 03"), Some(vec![0x01, 0x03]));
        assert!(hex_to_bytes("123").is_none());
    }
}
