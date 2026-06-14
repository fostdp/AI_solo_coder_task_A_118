use std::sync::Arc;

use anyhow::{Context, Result};
use chrono::{DateTime, Duration, Utc};
use clickhouse::Client;
use tracing::{debug, info, warn};

use crate::models::{
    AlarmEvent, ControlStep, FurnaceConfig, ProductionStats, SensorReading, ThermoParams,
};

#[derive(Clone)]
pub struct ClickHouseStore {
    client: Arc<Client>,
}

impl ClickHouseStore {
    pub fn new(url: &str, database: &str, username: &str, password: &str) -> Result<Self> {
        let mut client = Client::default().with_url(url);
        if !username.is_empty() {
            client = client.with_user(username);
        }
        if !password.is_empty() {
            client = client.with_password(password);
        }
        if !database.is_empty() {
            client = client.with_database(database);
        }

        Ok(Self {
            client: Arc::new(client),
        })
    }

    pub async fn insert_sensor_reading(&self, reading: &SensorReading) -> Result<()> {
        let mut insert = self.client.insert("sensor_data").with_context(|| {
            format!(
                "无法插入sensor_data记录: furnace={}",
                reading.furnace_id
            )
        })?;

        insert
            .write(&RowSensorData::from(reading))
            .await
            .with_context(|| "写入sensor_data行失败")?;

        insert.end().await.with_context(|| "结束sensor_data插入失败")?;

        debug!(
            "已存储传感器数据: furnace={}, temp={:.1}°C",
            reading.furnace_id, reading.furnace_temp
        );
        Ok(())
    }

    pub async fn insert_sensor_readings_batch(&self, readings: &[SensorReading]) -> Result<()> {
        if readings.is_empty() {
            return Ok(());
        }

        let mut insert = self.client.insert("sensor_data").with_context(|| {
            format!(
                "无法批量插入sensor_data: {}条",
                readings.len()
            )
        })?;

        for r in readings {
            insert
                .write(&RowSensorData::from(r))
                .await
                .with_context(|| "批量写入sensor_data行失败")?;
        }

        insert.end().await?;
        info!("批量存储传感器数据 {} 条", readings.len());
        Ok(())
    }

    pub async fn insert_alarm(&self, alarm: &AlarmEvent) -> Result<()> {
        let mut insert = self
            .client
            .insert("alarm_events")
            .with_context(|| "无法插入alarm_events记录")?;

        insert
            .write(&RowAlarm::from(alarm))
            .await
            .with_context(|| "写入alarm_events行失败")?;

        insert.end().await?;
        debug!(
            "已存储告警: furnace={}, type={:?}",
            alarm.furnace_id, alarm.alarm_type
        );
        Ok(())
    }

    pub async fn insert_control_step(&self, step: &ControlStep) -> Result<()> {
        let mut insert = self
            .client
            .insert("rl_control_actions")
            .with_context(|| "无法插入rl_control_actions记录")?;

        insert
            .write(&RowControlStep::from(step))
            .await
            .with_context(|| "写入rl_control_actions行失败")?;

        insert.end().await?;
        Ok(())
    }

    pub async fn insert_thermo_params(&self, params: &ThermoParams) -> Result<()> {
        let mut insert = self
            .client
            .insert("thermo_simulation_params")
            .with_context(|| "无法插入thermo_simulation_params记录")?;

        insert
            .write(&RowThermoParams::from(params))
            .await
            .with_context(|| "写入thermo_simulation_params行失败")?;

        insert.end().await?;
        Ok(())
    }

    pub async fn get_furnace_configs(&self) -> Result<Vec<FurnaceConfig>> {
        let rows = self
            .client
            .query(
                "SELECT furnace_id, furnace_name, furnace_type, volume_m3, 
                       max_temperature, target_temp_min, target_temp_max 
                 FROM furnaces 
                 ORDER BY furnace_id",
            )
            .fetch_all::<RowFurnace>()
            .await
            .with_context(|| "查询冶炼炉配置失败")?;

        Ok(rows.into_iter().map(FurnaceConfig::from).collect())
    }

    pub async fn get_furnace_config(&self, furnace_id: &str) -> Result<Option<FurnaceConfig>> {
        let row = self
            .client
            .query(
                "SELECT furnace_id, furnace_name, furnace_type, volume_m3, 
                       max_temperature, target_temp_min, target_temp_max 
                 FROM furnaces 
                 WHERE furnace_id = ?",
            )
            .bind(furnace_id)
            .fetch_optional::<RowFurnace>()
            .await
            .with_context(|| format!("查询冶炼炉配置失败: {}", furnace_id))?;

        Ok(row.map(FurnaceConfig::from))
    }

    pub async fn get_latest_reading(&self, furnace_id: &str) -> Result<Option<SensorReading>> {
        let row = self
            .client
            .query(
                "SELECT timestamp, furnace_id, push_pull_frequency, stroke_length, 
                       wind_pressure, air_volume, furnace_temp, co_concentration, 
                       o2_concentration, iron_feed_rate, coal_feed_rate, pig_iron_output,
                       temp_zone_top, temp_zone_upper, temp_zone_middle, 
                       temp_zone_lower, temp_zone_hearth, reaction_rate, 
                       energy_efficiency, quality, protocol
                 FROM sensor_data 
                 WHERE furnace_id = ? 
                 ORDER BY timestamp DESC 
                 LIMIT 1",
            )
            .bind(furnace_id)
            .fetch_optional::<RowSensorData>()
            .await
            .with_context(|| format!("查询最新传感器数据失败: {}", furnace_id))?;

        Ok(row.map(SensorReading::from))
    }

    pub async fn get_readings_range(
        &self,
        furnace_id: &str,
        start: DateTime<Utc>,
        end: DateTime<Utc>,
        limit: u64,
    ) -> Result<Vec<SensorReading>> {
        let rows = self
            .client
            .query(
                "SELECT timestamp, furnace_id, push_pull_frequency, stroke_length, 
                       wind_pressure, air_volume, furnace_temp, co_concentration, 
                       o2_concentration, iron_feed_rate, coal_feed_rate, pig_iron_output,
                       temp_zone_top, temp_zone_upper, temp_zone_middle, 
                       temp_zone_lower, temp_zone_hearth, reaction_rate, 
                       energy_efficiency, quality, protocol
                 FROM sensor_data 
                 WHERE furnace_id = ? AND timestamp >= ? AND timestamp <= ?
                 ORDER BY timestamp ASC 
                 LIMIT ?",
            )
            .bind(furnace_id)
            .bind(start)
            .bind(end)
            .bind(limit)
            .fetch_all::<RowSensorData>()
            .await
            .with_context(|| format!("查询传感器历史数据失败: {}", furnace_id))?;

        Ok(rows.into_iter().map(SensorReading::from).collect())
    }

    pub async fn get_readings_last_n(&self, furnace_id: &str, n: u64) -> Result<Vec<SensorReading>> {
        let rows = self
            .client
            .query(
                "SELECT timestamp, furnace_id, push_pull_frequency, stroke_length, 
                       wind_pressure, air_volume, furnace_temp, co_concentration, 
                       o2_concentration, iron_feed_rate, coal_feed_rate, pig_iron_output,
                       temp_zone_top, temp_zone_upper, temp_zone_middle, 
                       temp_zone_lower, temp_zone_hearth, reaction_rate, 
                       energy_efficiency, quality, protocol
                 FROM (
                     SELECT *
                     FROM sensor_data 
                     WHERE furnace_id = ? 
                     ORDER BY timestamp DESC 
                     LIMIT ?
                 ) ORDER BY timestamp ASC",
            )
            .bind(furnace_id)
            .bind(n)
            .fetch_all::<RowSensorData>()
            .await
            .with_context(|| format!("查询最近N条传感器数据失败: {}", furnace_id))?;

        Ok(rows.into_iter().map(SensorReading::from).collect())
    }

    pub async fn get_thermo_params(&self, furnace_id: &str) -> Result<Option<ThermoParams>> {
        let row = self
            .client
            .query(
                "SELECT furnace_id, heat_conductivity, specific_heat, reaction_enthalpy,
                       activation_energy, pre_exponential_factor, heat_loss_coefficient, 
                       air_preheat_temp
                 FROM thermo_simulation_params 
                 WHERE furnace_id = ? 
                 ORDER BY timestamp DESC 
                 LIMIT 1",
            )
            .bind(furnace_id)
            .fetch_optional::<RowThermoParams>()
            .await
            .with_context(|| format!("查询热力学参数失败: {}", furnace_id))?;

        Ok(row.map(ThermoParams::from))
    }

    pub async fn get_active_alarms(
        &self,
        furnace_id: Option<&str>,
        hours: u32,
    ) -> Result<Vec<AlarmEvent>> {
        let since = Utc::now() - Duration::hours(hours as i64);

        let rows = match furnace_id {
            Some(fid) => {
                self.client
                    .query(
                        "SELECT event_id, timestamp, furnace_id, alarm_type, alarm_level, 
                               message, current_value, threshold_value, acknowledged, mqtt_published
                         FROM alarm_events 
                         WHERE furnace_id = ? AND timestamp >= ? AND acknowledged = 0
                         ORDER BY timestamp DESC 
                         LIMIT 100",
                    )
                    .bind(fid)
                    .bind(since)
                    .fetch_all::<RowAlarm>()
                    .await?
            }
            None => {
                self.client
                    .query(
                        "SELECT event_id, timestamp, furnace_id, alarm_type, alarm_level, 
                               message, current_value, threshold_value, acknowledged, mqtt_published
                         FROM alarm_events 
                         WHERE timestamp >= ? AND acknowledged = 0
                         ORDER BY timestamp DESC 
                         LIMIT 100",
                    )
                    .bind(since)
                    .fetch_all::<RowAlarm>()
                    .await?
            }
        };

        Ok(rows.into_iter().map(AlarmEvent::from).collect())
    }

    pub async fn get_production_stats(
        &self,
        furnace_id: &str,
        days: u32,
    ) -> Result<Vec<ProductionStats>> {
        let since = chrono::Local::now().date_naive() - Duration::days(days as i64);

        let rows = self
            .client
            .query(
                "SELECT stat_date, furnace_id, total_iron_kg, total_coal_kg, 
                       total_iron_ore_kg, avg_temp, avg_co_concentration, 
                       avg_energy_efficiency, operation_hours, alarm_count
                 FROM iron_production_stats 
                 WHERE furnace_id = ? AND stat_date >= ?
                 ORDER BY stat_date ASC
                 LIMIT ?",
            )
            .bind(furnace_id)
            .bind(since)
            .bind(days)
            .fetch_all::<RowProductionStats>()
            .await
            .with_context(|| format!("查询产量统计失败: {}", furnace_id))?;

        Ok(rows.into_iter().map(ProductionStats::from).collect())
    }

    pub async fn acknowledge_alarm(&self, event_id: &str) -> Result<u64> {
        let res = self
            .client
            .query(
                "ALTER TABLE alarm_events 
                 UPDATE acknowledged = 1 
                 WHERE toString(event_id) = ?",
            )
            .bind(event_id)
            .execute()
            .await;

        match res {
            Ok(_) => Ok(1),
            Err(e) => {
                warn!("确认告警失败: {}, event_id={}", e, event_id);
                Ok(0)
            }
        }
    }

    pub async fn ping(&self) -> Result<bool> {
        let _: Vec<u8> = self
            .client
            .query("SELECT 1")
            .fetch_all()
            .await
            .map_err(|e| anyhow::anyhow!("ClickHouse连接失败: {}", e))?;
        Ok(true)
    }
}

#[derive(clickhouse::Row, serde::Serialize, serde::Deserialize, Debug)]
struct RowSensorData {
    timestamp: DateTime<Utc>,
    furnace_id: String,
    push_pull_frequency: f64,
    stroke_length: f64,
    wind_pressure: f64,
    air_volume: f64,
    furnace_temp: f64,
    co_concentration: f64,
    o2_concentration: f64,
    iron_feed_rate: f64,
    coal_feed_rate: f64,
    pig_iron_output: f64,
    temp_zone_top: f64,
    temp_zone_upper: f64,
    temp_zone_middle: f64,
    temp_zone_lower: f64,
    temp_zone_hearth: f64,
    reaction_rate: f64,
    energy_efficiency: f64,
    quality: f64,
    protocol: String,
}

impl<'a> From<&'a SensorReading> for RowSensorData {
    fn from(r: &'a SensorReading) -> Self {
        Self {
            timestamp: r.timestamp,
            furnace_id: r.furnace_id.clone(),
            push_pull_frequency: r.push_pull_frequency,
            stroke_length: r.stroke_length,
            wind_pressure: r.wind_pressure,
            air_volume: r.air_volume,
            furnace_temp: r.furnace_temp,
            co_concentration: r.co_concentration,
            o2_concentration: r.o2_concentration,
            iron_feed_rate: r.iron_feed_rate,
            coal_feed_rate: r.coal_feed_rate,
            pig_iron_output: r.pig_iron_output,
            temp_zone_top: r.temp_zone_top,
            temp_zone_upper: r.temp_zone_upper,
            temp_zone_middle: r.temp_zone_middle,
            temp_zone_lower: r.temp_zone_lower,
            temp_zone_hearth: r.temp_zone_hearth,
            reaction_rate: r.reaction_rate,
            energy_efficiency: r.energy_efficiency,
            quality: r.quality,
            protocol: r.protocol.clone(),
        }
    }
}

impl From<RowSensorData> for SensorReading {
    fn from(r: RowSensorData) -> Self {
        Self {
            timestamp: r.timestamp,
            furnace_id: r.furnace_id,
            push_pull_frequency: r.push_pull_frequency,
            stroke_length: r.stroke_length,
            wind_pressure: r.wind_pressure,
            air_volume: r.air_volume,
            furnace_temp: r.furnace_temp,
            co_concentration: r.co_concentration,
            o2_concentration: r.o2_concentration,
            iron_feed_rate: r.iron_feed_rate,
            coal_feed_rate: r.coal_feed_rate,
            pig_iron_output: r.pig_iron_output,
            temp_zone_top: r.temp_zone_top,
            temp_zone_upper: r.temp_zone_upper,
            temp_zone_middle: r.temp_zone_middle,
            temp_zone_lower: r.temp_zone_lower,
            temp_zone_hearth: r.temp_zone_hearth,
            reaction_rate: r.reaction_rate,
            energy_efficiency: r.energy_efficiency,
            quality: r.quality,
            protocol: r.protocol,
            phase: None,
            modbus_frame_hex: None,
        }
    }
}

#[derive(clickhouse::Row, serde::Serialize, serde::Deserialize, Debug)]
struct RowFurnace {
    furnace_id: String,
    furnace_name: String,
    furnace_type: String,
    volume_m3: f64,
    max_temperature: f64,
    target_temp_min: f64,
    target_temp_max: f64,
}

impl From<RowFurnace> for FurnaceConfig {
    fn from(r: RowFurnace) -> Self {
        Self {
            furnace_id: r.furnace_id,
            furnace_name: r.furnace_name,
            furnace_type: crate::models::FurnaceType::from_str(&r.furnace_type)
                .unwrap_or_default(),
            volume_m3: r.volume_m3,
            max_temperature: r.max_temperature,
            target_temp_min: r.target_temp_min,
            target_temp_max: r.target_temp_max,
        }
    }
}

#[derive(clickhouse::Row, serde::Serialize, serde::Deserialize, Debug)]
struct RowAlarm {
    event_id: Uuid,
    timestamp: DateTime<Utc>,
    furnace_id: String,
    alarm_type: String,
    alarm_level: String,
    message: String,
    current_value: f64,
    threshold_value: f64,
    acknowledged: u8,
    mqtt_published: u8,
}

impl<'a> From<&'a AlarmEvent> for RowAlarm {
    fn from(a: &'a AlarmEvent) -> Self {
        Self {
            event_id: a.event_id,
            timestamp: a.timestamp,
            furnace_id: a.furnace_id.clone(),
            alarm_type: a.alarm_type.as_str().to_string(),
            alarm_level: a.alarm_level.as_str().to_string(),
            message: a.message.clone(),
            current_value: a.current_value,
            threshold_value: a.threshold_value,
            acknowledged: a.acknowledged,
            mqtt_published: a.mqtt_published,
        }
    }
}

fn parse_alarm_type(s: &str) -> crate::models::AlarmType {
    use crate::models::AlarmType::*;
    match s {
        "TEMP_TOO_HIGH" => TempTooHigh,
        "TEMP_TOO_LOW" => TempTooLow,
        "CO_ACCUMULATION" => CoAccumulation,
        "PRESSURE_ABNORMAL" => PressureAbnormal,
        "EFFICIENCY_LOW" => EfficiencyLow,
        _ => SystemError,
    }
}

fn parse_alarm_level(s: &str) -> crate::models::AlarmLevel {
    use crate::models::AlarmLevel::*;
    match s {
        "WARNING" => Warning,
        "CRITICAL" => Critical,
        "FATAL" => Fatal,
        _ => Warning,
    }
}

impl From<RowAlarm> for AlarmEvent {
    fn from(r: RowAlarm) -> Self {
        Self {
            event_id: r.event_id,
            timestamp: r.timestamp,
            furnace_id: r.furnace_id,
            alarm_type: parse_alarm_type(&r.alarm_type),
            alarm_level: parse_alarm_level(&r.alarm_level),
            message: r.message,
            current_value: r.current_value,
            threshold_value: r.threshold_value,
            acknowledged: r.acknowledged,
            mqtt_published: r.mqtt_published,
        }
    }
}

#[derive(clickhouse::Row, serde::Serialize, serde::Deserialize, Debug)]
struct RowControlStep {
    timestamp: DateTime<Utc>,
    furnace_id: String,
    episode: u32,
    step: u32,
    state_vector: Vec<f64>,
    action_frequency: f64,
    action_stroke: f64,
    reward: f64,
    next_state_vector: Vec<f64>,
    done: u8,
    loss: f64,
    epsilon: f64,
    learning_rate: f64,
}

impl<'a> From<&'a ControlStep> for RowControlStep {
    fn from(c: &'a ControlStep) -> Self {
        Self {
            timestamp: c.timestamp,
            furnace_id: c.furnace_id.clone(),
            episode: c.episode,
            step: c.step,
            state_vector: c.state_vector.clone(),
            action_frequency: c.action_frequency,
            action_stroke: c.action_stroke,
            reward: c.reward,
            next_state_vector: c.next_state_vector.clone(),
            done: c.done,
            loss: c.loss,
            epsilon: c.epsilon,
            learning_rate: c.learning_rate,
        }
    }
}

#[derive(clickhouse::Row, serde::Serialize, serde::Deserialize, Debug)]
struct RowThermoParams {
    furnace_id: String,
    heat_conductivity: f64,
    specific_heat: f64,
    reaction_enthalpy: f64,
    activation_energy: f64,
    pre_exponential_factor: f64,
    heat_loss_coefficient: f64,
    air_preheat_temp: f64,
}

impl<'a> From<&'a ThermoParams> for RowThermoParams {
    fn from(p: &'a ThermoParams) -> Self {
        Self {
            furnace_id: p.furnace_id.clone(),
            heat_conductivity: p.heat_conductivity,
            specific_heat: p.specific_heat,
            reaction_enthalpy: p.reaction_enthalpy,
            activation_energy: p.activation_energy,
            pre_exponential_factor: p.pre_exponential_factor,
            heat_loss_coefficient: p.heat_loss_coefficient,
            air_preheat_temp: p.air_preheat_temp,
        }
    }
}

impl From<RowThermoParams> for ThermoParams {
    fn from(r: RowThermoParams) -> Self {
        Self {
            furnace_id: r.furnace_id,
            heat_conductivity: r.heat_conductivity,
            specific_heat: r.specific_heat,
            reaction_enthalpy: r.reaction_enthalpy,
            activation_energy: r.activation_energy,
            pre_exponential_factor: r.pre_exponential_factor,
            heat_loss_coefficient: r.heat_loss_coefficient,
            air_preheat_temp: r.air_preheat_temp,
        }
    }
}

#[derive(clickhouse::Row, serde::Serialize, serde::Deserialize, Debug)]
struct RowProductionStats {
    stat_date: chrono::NaiveDate,
    furnace_id: String,
    total_iron_kg: f64,
    total_coal_kg: f64,
    total_iron_ore_kg: f64,
    avg_temp: f64,
    avg_co_concentration: f64,
    avg_energy_efficiency: f64,
    operation_hours: f64,
    alarm_count: u32,
}

impl From<RowProductionStats> for ProductionStats {
    fn from(r: RowProductionStats) -> Self {
        Self {
            stat_date: r.stat_date.format("%Y-%m-%d").to_string(),
            furnace_id: r.furnace_id,
            total_iron_kg: r.total_iron_kg,
            total_coal_kg: r.total_coal_kg,
            total_iron_ore_kg: r.total_iron_ore_kg,
            avg_temp: r.avg_temp,
            avg_co_concentration: r.avg_co_concentration,
            avg_energy_efficiency: r.avg_energy_efficiency,
            operation_hours: r.operation_hours,
            alarm_count: r.alarm_count,
        }
    }
}
