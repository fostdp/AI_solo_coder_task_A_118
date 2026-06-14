use std::collections::HashMap;
use std::sync::Arc;

use once_cell::sync::Lazy;
use serde::{Deserialize, Serialize};
use tokio::sync::{mpsc, RwLock};
use tracing::{debug, error, info, warn};

use crate::config::{SystemConfig, ThermodynamicsConfig};
use crate::models::{FurnaceConfig, SensorReading, ThermoParams, ThermoPrediction};
use crate::modbus_receiver::ValidatedReading;
use crate::parameter_id::{IdentifiedParams, MultiFurnaceIdentifier, OnlineParameterIdentifier};
use crate::thermodynamics::{MultiFurnaceThermoEngine, ThermodynamicsEngine};

use std::sync::Mutex as StdMutex;

pub(crate) static LAST_READINGS_CACHE: Lazy<StdMutex<HashMap<String, SensorReading>>> =
    Lazy::new(|| StdMutex::new(HashMap::new()));

pub fn get_cached_reading(furnace_id: &str) -> Option<SensorReading> {
    let cache = LAST_READINGS_CACHE.lock().ok()?;
    cache.get(furnace_id).cloned()
}

pub fn update_cached_reading(furnace_id: &str, reading: &SensorReading) {
    if let Ok(mut cache) = LAST_READINGS_CACHE.lock() {
        cache.insert(furnace_id.to_string(), reading.clone());
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum ThermoRequest {
    ProcessReading {
        reading: ValidatedReading,
    },
    Predict {
        furnace_id: String,
        proposed_frequency: f64,
        proposed_stroke: f64,
        dt: f64,
    },
    GetTempField {
        furnace_id: String,
        resolution: (usize, usize),
    },
    GetParams {
        furnace_id: String,
    },
    UpdateParams {
        furnace_id: String,
        params: ThermoParams,
    },
    GetIdentifiedParams {
        furnace_id: String,
    },
    GetEngineStatus,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum ThermoResponse {
    ReadingProcessed {
        furnace_id: String,
        predicted: ThermoPrediction,
        identified: Option<IdentifiedParams>,
    },
    Prediction(ThermoPrediction),
    TempField(TempFieldResult),
    Params(ThermoParams),
    ParamsUpdated,
    IdentifiedParams(Option<IdentifiedParams>),
    EngineStatus(EngineStatusReport),
    Error {
        code: String,
        message: String,
    },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TempFieldResult {
    pub furnace_id: String,
    pub resolution: (usize, usize),
    pub temp_min: f64,
    pub temp_max: f64,
    pub zones: [f64; 5],
    pub field_data: Vec<Vec<f64>>,
    pub timestamp: chrono::DateTime<chrono::Utc>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct EngineStatusReport {
    pub total_processed: u64,
    pub prediction_errors: u64,
    pub param_id_active: bool,
    pub furnaces: HashMap<String, FurnaceEngineStatus>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct FurnaceEngineStatus {
    pub processed: u64,
    pub last_temp: f64,
    pub avg_temp: f64,
    pub param_confidence: f64,
    pub param_id_enabled: bool,
    pub last_processed_at: Option<chrono::DateTime<chrono::Utc>>,
}

pub struct ThermodynamicsSimulator {
    config: Arc<SystemConfig>,
    thermo_cfg: ThermodynamicsConfig,
    engine: RwLock<MultiFurnaceThermoEngine>,
    param_identifier: RwLock<MultiFurnaceIdentifier>,
    status: RwLock<EngineStatusReport>,
}

impl ThermodynamicsSimulator {
    pub fn new(config: Arc<SystemConfig>, furnace_cfgs: Vec<(FurnaceConfig, ThermoParams)>) -> Self {
        let mut engine = MultiFurnaceThermoEngine::new();
        let mut identifier = MultiFurnaceIdentifier::new();
        for (fc, tp) in &furnace_cfgs {
            engine.add_furnace(fc.clone(), tp.clone());
            identifier.add_furnace(
                fc.furnace_id.clone(),
                (tp.activation_energy, tp.pre_exponential_factor, tp.heat_loss_coefficient),
            );
        }

        let thermo_cfg = config.thermodynamics.clone();
        let status = EngineStatusReport {
            param_id_active: thermo_cfg.param_id_enabled,
            ..Default::default()
        };

        Self {
            config,
            thermo_cfg,
            engine: RwLock::new(engine),
            param_identifier: RwLock::new(identifier),
            status: RwLock::new(status),
        }
    }

    pub async fn start(
        self,
        mut req_rx: mpsc::Receiver<ThermoRequest>,
        resp_tx: mpsc::Sender<ThermoResponse>,
        mut validated_rx: mpsc::Receiver<ValidatedReading>,
        post_thermo_tx: mpsc::Sender<ValidatedReading>,
    ) {
        info!(
            "ThermodynamicsSimulator 启动, 预测={}, 参数辨识={}, 分辨率={}",
            self.thermo_cfg.prediction_enabled,
            self.thermo_cfg.param_id_enabled,
            self.thermo_cfg.temp_field_resolution
        );

        let validated_sender = post_thermo_tx.clone();
        let self_arc = Arc::new(self);

        let sim_self = self_arc.clone();
        let resp_tx_inner = resp_tx.clone();
        tokio::spawn(async move {
            while let Some(validated) = validated_rx.recv().await {
                let furnace_id = validated.reading.furnace_id.clone();
                let reading = validated.reading.clone();

                let identified = if sim_self.thermo_cfg.param_id_enabled {
                    let mut id = sim_self.param_identifier.write().await;
                    id.process_reading(&furnace_id, &reading, sim_self.thermo_cfg.predict_dt_secs)
                } else {
                    None
                };

                if let Some(id) = &identified {
                    let mut engine = sim_self.engine.write().await;
                    if let Some(e) = engine.get_engine_mut(&furnace_id) {
                        e.apply_identified_params(id);
                    }
                }

                let predicted = if sim_self.thermo_cfg.prediction_enabled {
                    let mut engine = sim_self.engine.write().await;
                    match engine.get_engine_mut(&furnace_id) {
                        Some(e) => {
                            e.update_with_reading(&reading);
                            e.predict_next(
                                &reading,
                                reading.push_pull_frequency,
                                reading.stroke_length,
                                sim_self.thermo_cfg.predict_dt_secs,
                            )
                        }
                        None => {
                            warn!("热力学引擎未初始化: {}", furnace_id);
                            ThermoPrediction {
                                timestamp: chrono::Utc::now(),
                                furnace_id: furnace_id.clone(),
                                predicted_temp: reading.furnace_temp,
                                predicted_co: reading.co_concentration,
                                predicted_reaction_rate: reading.reaction_rate,
                                predicted_efficiency: reading.energy_efficiency,
                                temp_distribution: reading.temp_zones().to_vec(),
                                iron_output_rate: reading.pig_iron_output,
                                confidence: 0.0,
                            }
                        }
                    }
                } else {
                    ThermoPrediction {
                        timestamp: chrono::Utc::now(),
                        furnace_id: furnace_id.clone(),
                        predicted_temp: reading.furnace_temp,
                        predicted_co: reading.co_concentration,
                        predicted_reaction_rate: reading.reaction_rate,
                        predicted_efficiency: reading.energy_efficiency,
                        temp_distribution: reading.temp_zones().to_vec(),
                        iron_output_rate: reading.pig_iron_output,
                        confidence: 0.0,
                    }
                };

                {
                    let mut status = sim_self.status.write().await;
                    status.total_processed += 1;
                    let fst = status
                        .furnaces
                        .entry(furnace_id.clone())
                        .or_default();
                    fst.processed += 1;
                    fst.last_temp = reading.furnace_temp;
                    fst.param_confidence = identified.as_ref().map(|i| i.confidence).unwrap_or(0.0);
                    fst.param_id_enabled = sim_self.thermo_cfg.param_id_enabled;
                    fst.last_processed_at = Some(chrono::Utc::now());
                    let n = fst.processed as f64;
                    fst.avg_temp = fst.avg_temp * (n - 1.0) / n + reading.furnace_temp / n;
                }

                let algo = if sim_self.thermo_cfg.param_id_enabled {
                    "arrhenius+rls" } else { "arrhenius" };
                crate::metrics::inc_thermo_predictions(&furnace_id, algo);

                let resp = ThermoResponse::ReadingProcessed {
                    furnace_id: furnace_id.clone(),
                    predicted: predicted.clone(),
                    identified: identified.clone(),
                };
                if let Err(e) = resp_tx_inner.send(resp).await {
                    error!("发送热力学响应失败: {}", e);
                }

                let mut post = validated.clone();
                post.reading.energy_efficiency = predicted.predicted_efficiency;
                post.reading.pig_iron_output = predicted.iron_output_rate;
                post.reading.reaction_rate = predicted.predicted_reaction_rate;

                if let Err(e) = validated_sender.send(post).await {
                    warn!("post_thermo 通道发送失败: {}", e);
                }
            }
        });

        while let Some(req) = req_rx.recv().await {
            let resp = self_arc.handle_request(req).await;
            if let Err(e) = resp_tx.send(resp).await {
                error!("发送热力学响应失败: {}", e);
                break;
            }
        }
        info!("ThermodynamicsSimulator 退出");
    }

    async fn handle_request(&self, req: ThermoRequest) -> ThermoResponse {
        match req {
            ThermoRequest::ProcessReading { reading } => {
                let furnace_id = reading.reading.furnace_id.clone();
                let r = reading.reading.clone();
                let mut engine = self.engine.write().await;
                match engine.get_engine_mut(&furnace_id) {
                    Some(e) => {
                        e.update_with_reading(&r);
                        let pred = e.predict_next(
                            &r,
                            r.push_pull_frequency,
                            r.stroke_length,
                            self.thermo_cfg.predict_dt_secs,
                        );
                        ThermoResponse::ReadingProcessed {
                            furnace_id,
                            predicted: pred,
                            identified: None,
                        }
                    }
                    None => ThermoResponse::Error {
                        code: "NO_ENGINE".into(),
                        message: format!("未找到炉 '{}' 的热力学引擎", furnace_id),
                    },
                }
            }
            ThermoRequest::Predict {
                furnace_id,
                proposed_frequency,
                proposed_stroke,
                dt,
            } => {
                let reading = get_cached_reading(&furnace_id);
                let Some(reading) = reading else {
                    return ThermoResponse::Error {
                        code: "NO_READING".into(),
                        message: "暂无最近传感器数据".into(),
                    };
                };
                let mut engine = self.engine.write().await;
                match engine.get_engine_mut(&furnace_id) {
                    Some(e) => ThermoResponse::Prediction(
                        e.predict_next(&reading, proposed_frequency, proposed_stroke, dt),
                    ),
                    None => ThermoResponse::Error {
                        code: "NO_ENGINE".into(),
                        message: "未找到热力学引擎".into(),
                    },
                }
            }
            ThermoRequest::GetTempField {
                furnace_id,
                resolution,
            } => {
                let reading = get_cached_reading(&furnace_id);
                let Some(reading) = reading else {
                    return ThermoResponse::Error {
                        code: "NO_READING".into(),
                        message: "暂无传感器数据".into(),
                    };
                };

                let zones = reading.temp_zones();
                let temp_min = zones.iter().cloned().fold(f64::INFINITY, f64::min) - 20.0;
                let temp_max = zones.iter().cloned().fold(f64::NEG_INFINITY, f64::max) + 20.0;

                let engine = self.engine.read().await;
                let field = match engine.get_engine(&furnace_id) {
                    Some(e) => e.simulate_temp_field(zones, resolution),
                    None => {
                        let mut basic = ndarray::Array2::zeros(resolution);
                        for r in 0..resolution.0 {
                            for c in 0..resolution.1 {
                                let zone_idx = ((r as f64 / resolution.0 as f64) * 5.0) as usize;
                                let zone_idx = zone_idx.min(4);
                                basic[[r, c]] = zones[zone_idx];
                            }
                        }
                        basic
                    }
                };

                let mut field_data = Vec::with_capacity(resolution.0);
                for r in 0..resolution.0 {
                    let mut row = Vec::with_capacity(resolution.1);
                    for c in 0..resolution.1 {
                        row.push(field[[r, c]]);
                    }
                    field_data.push(row);
                }

                ThermoResponse::TempField(TempFieldResult {
                    furnace_id,
                    resolution,
                    temp_min,
                    temp_max,
                    zones,
                    field_data,
                    timestamp: reading.timestamp,
                })
            }
            ThermoRequest::GetParams { furnace_id } => {
                let engine = self.engine.read().await;
                match engine.get_engine(&furnace_id).map(|e| e.get_params().clone()) {
                    Some(p) => ThermoResponse::Params(p),
                    None => ThermoResponse::Error {
                        code: "NO_ENGINE".into(),
                        message: "未找到引擎".into(),
                    },
                }
            }
            ThermoRequest::UpdateParams { furnace_id, params } => {
                let mut engine = self.engine.write().await;
                if let Some(e) = engine.get_engine_mut(&furnace_id) {
                    e.update_params(params);
                    ThermoResponse::ParamsUpdated
                } else {
                    ThermoResponse::Error {
                        code: "NO_ENGINE".into(),
                        message: "未找到引擎".into(),
                    }
                }
            }
            ThermoRequest::GetIdentifiedParams { furnace_id } => {
                let id = self.param_identifier.read().await;
                ThermoResponse::IdentifiedParams(id.get_params(&furnace_id))
            }
            ThermoRequest::GetEngineStatus => {
                ThermoResponse::EngineStatus(self.status.read().await.clone())
            }
        }
    }
}

