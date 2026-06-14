use std::collections::HashMap;
use std::sync::Arc;

use serde::{Deserialize, Serialize};
use tokio::sync::{mpsc, Mutex, RwLock};
use tracing::{debug, error, info, warn};

use crate::config::{ControlAlgorithm, SystemConfig};
use crate::models::{FurnaceConfig, FurnaceType, RLAction, RLControlStep, SensorReading};
use crate::modbus_receiver::ValidatedReading;
use crate::qlearning::{MultiFurnaceQLController, QLearningController, QLearningStatus};
use crate::rl_control::{MultiFurnaceRLController, RLStatus};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum ControlRequest {
    ProcessReading {
        reading: ValidatedReading,
    },
    GetStatus {
        furnace_id: Option<String>,
    },
    GetAction {
        furnace_id: String,
    },
    SetManualAction {
        furnace_id: String,
        action: ManualOverride,
    },
    ClearManualOverride {
        furnace_id: String,
    },
    SwitchAlgorithm {
        algo: ControlAlgorithm,
    },
    ResetController {
        furnace_id: String,
    },
    GetAlgo,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ManualOverride {
    pub frequency: Option<f64>,
    pub stroke: Option<f64>,
    pub expires_at: Option<chrono::DateTime<chrono::Utc>>,
    pub reason: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum ControlResponse {
    ActionComputed {
        furnace_id: String,
        action: RLAction,
        algo: ControlAlgorithm,
        manual_override: bool,
        step: Option<RLControlStep>,
    },
    StatusAll {
        current_algo: ControlAlgorithm,
        ql_statuses: Vec<(String, QLearningStatus)>,
        ddpg_statuses: Vec<(String, RLStatus)>,
    },
    StatusSingle {
        furnace_id: String,
        current_algo: ControlAlgorithm,
        ql: Option<QLearningStatus>,
        ddpg: Option<RLStatus>,
    },
    AlgoSwitched {
        from: ControlAlgorithm,
        to: ControlAlgorithm,
    },
    ControllerReset {
        furnace_id: String,
        algo: ControlAlgorithm,
    },
    ManualOverrideApplied {
        furnace_id: String,
        action: RLAction,
        expires_at: Option<chrono::DateTime<chrono::Utc>>,
    },
    ManualOverrideCleared {
        furnace_id: String,
    },
    AlgoInfo {
        algo: ControlAlgorithm,
    },
    Error {
        code: String,
        message: String,
    },
}

pub struct ControlOptimizer {
    config: Arc<SystemConfig>,
    current_algo: RwLock<ControlAlgorithm>,
    ql_controller: RwLock<MultiFurnaceQLController>,
    ddpg_controller: MultiFurnaceRLController,
    manual_overrides: RwLock<HashMap<String, ManualOverride>>,
    furnace_configs: HashMap<String, FurnaceConfig>,
    action_history: Mutex<HashMap<String, Vec<RLAction>>>,
}

impl ControlOptimizer {
    pub fn new(
        config: Arc<SystemConfig>,
        furnace_configs: Vec<FurnaceConfig>,
    ) -> Self {
        let mut ql = MultiFurnaceQLController::new();
        let mut ddpg = MultiFurnaceRLController::new();
        let mut fmap = HashMap::new();

        for fc in furnace_configs {
            ql.add_furnace(fc.furnace_id.clone(), fc.clone());
            ddpg.add_furnace(fc.furnace_id.clone());
            fmap.insert(fc.furnace_id.clone(), fc);
        }

        let algo = config.control.default_algo;

        Self {
            config,
            current_algo: RwLock::new(algo),
            ql_controller: RwLock::new(ql),
            ddpg_controller,
            manual_overrides: RwLock::new(HashMap::new()),
            furnace_configs: fmap,
            action_history: Mutex::new(HashMap::new()),
        }
    }

    pub async fn start(
        self,
        mut validated_rx: mpsc::Receiver<ValidatedReading>,
        mut req_rx: mpsc::Receiver<ControlRequest>,
        resp_tx: mpsc::Sender<ControlResponse>,
        post_control_tx: mpsc::Sender<ControlResponse>,
    ) {
        let algo = *self.current_algo.read().await;
        info!(
            "ControlOptimizer 启动, 默认算法: {:?}, 炉数: {}",
            algo,
            self.furnace_configs.len()
        );

        let self_arc = Arc::new(self);
        let post_tx = post_control_tx.clone();

        let self_clone = self_arc.clone();
        tokio::spawn(async move {
            while let Some(validated) = validated_rx.recv().await {
                let furnace_id = validated.reading.furnace_id.clone();
                let reading = validated.reading.clone();

                let manual = self_clone.check_manual_override(&furnace_id, &reading).await;

                if let Some(mut action) = manual {
                    action.timestamp = reading.timestamp;
                    let resp = ControlResponse::ActionComputed {
                        furnace_id,
                        action,
                        algo: *self_clone.current_algo.read().await,
                        manual_override: true,
                        step: None,
                    };

                    self_clone.record_action(&reading.furnace_id, &resp).await;
                    crate::metrics::inc_control_actions(&reading.furnace_id, "manual");

                    if let Err(e) = post_tx.send(resp).await {
                        warn!("post_control 手动动作发送失败: {}", e);
                    }
                } else {
                    let algo = *self_clone.current_algo.read().await;
                    let (action, step) = match algo {
                        ControlAlgorithm::QLearning => {
                            let mut ql = self_clone.ql_controller.write().await;
                            let action = ql.select_action(&furnace_id, &reading);
                            let step = action.as_ref().map(|a| build_ql_step(&furnace_id, a, &reading));
                            (action, step)
                        }
                        ControlAlgorithm::Ddpg => {
                            let fc = self_clone.furnace_configs.get(&furnace_id).cloned();
                            match fc {
                                Some(cfg) => {
                                    self_clone
                                        .ddpg_controller
                                        .process_reading(&reading, &cfg, reading.furnace_temp)
                                }
                                None => (
                                    Some(RLAction {
                                        frequency: 25.0,
                                        stroke: 35.0,
                                        timestamp: reading.timestamp,
                                        q_value: None,
                                    }),
                                    None,
                                ),
                            }
                        }
                    };

                    let action = action.unwrap_or(RLAction {
                        frequency: 25.0,
                        stroke: 35.0,
                        timestamp: reading.timestamp,
                        q_value: None,
                    });

                    let resp = ControlResponse::ActionComputed {
                        furnace_id,
                        action,
                        algo,
                        manual_override: false,
                        step,
                    };

                    self_clone.record_action(&reading.furnace_id, &resp).await;
                    let mode = match algo {
                        ControlAlgorithm::QLearning => "qlearning",
                        ControlAlgorithm::Ddpg => "ddpg",
                    };
                    crate::metrics::inc_control_actions(&reading.furnace_id, mode);

                    if let Err(e) = post_tx.send(resp).await {
                        warn!("post_control 计算动作发送失败: {}", e);
                    }
                }
            }
            info!("控制优化validated_rx退出");
        });

        while let Some(req) = req_rx.recv().await {
            let resp = self_arc.handle_request(req).await;
            if let Err(e) = resp_tx.send(resp).await {
                error!("发送控制响应失败: {}", e);
                break;
            }
        }
        info!("ControlOptimizer 退出");
    }

    async fn check_manual_override(
        &self,
        furnace_id: &str,
        reading: &SensorReading,
    ) -> Option<RLAction> {
        let ov = self.manual_overrides.read().await;
        let entry = ov.get(furnace_id)?;
        if let Some(exp) = entry.expires_at {
            if exp < reading.timestamp {
                return None;
            }
        }
        let base_f = match self.furnace_configs.get(furnace_id) {
            Some(fc) => match fc.furnace_type {
                FurnaceType::HanChaogang => 25.0,
                FurnaceType::MingBlast => 32.0,
            },
            _ => 25.0,
        };
        let base_s = match self.furnace_configs.get(furnace_id) {
            Some(fc) => match fc.furnace_type {
                FurnaceType::HanChaogang => 35.0,
                FurnaceType::MingBlast => 50.0,
            },
            _ => 35.0,
        };
        Some(RLAction {
            frequency: entry.frequency.unwrap_or(base_f),
            stroke: entry.stroke.unwrap_or(base_s),
            timestamp: reading.timestamp,
            q_value: None,
        })
    }

    async fn record_action(&self, furnace_id: &str, resp: &ControlResponse) {
        let action = match resp {
            ControlResponse::ActionComputed { action, .. } => action.clone(),
            _ => return,
        };
        let mut hist = self.action_history.lock().await;
        let list = hist.entry(furnace_id.to_string()).or_default();
        list.push(action);
        if list.len() > 100 {
            list.drain(0..list.len() - 100);
        }
    }

    async fn handle_request(&self, req: ControlRequest) -> ControlResponse {
        match req {
            ControlRequest::ProcessReading { .. } => ControlResponse::Error {
                code: "DEPRECATED".into(),
                message: "请使用 post_control 通道发送 ValidatedReading".into(),
            },
            ControlRequest::GetStatus { furnace_id: None } => {
                let algo = *self.current_algo.read().await;
                let ql = self.ql_controller.read().await.all_statuses();
                let ddpg = self.ddpg_controller.get_all_status();
                ControlResponse::StatusAll {
                    current_algo: algo,
                    ql_statuses: ql,
                    ddpg_statuses: ddpg.into_iter().map(|s| (s.furnace_id.clone(), s)).collect(),
                }
            }
            ControlRequest::GetStatus {
                furnace_id: Some(fid),
            } => {
                let algo = *self.current_algo.read().await;
                let ql = self.ql_controller.read().await.get_status(&fid);
                let ddpg = self.ddpg_controller.get_status(&fid);
                ControlResponse::StatusSingle {
                    furnace_id: fid,
                    current_algo: algo,
                    ql,
                    ddpg,
                }
            }
            ControlRequest::GetAction { furnace_id } => {
                let hist = self.action_history.lock().await;
                match hist.get(&furnace_id).and_then(|v| v.last().cloned()) {
                    Some(action) => ControlResponse::ActionComputed {
                        furnace_id,
                        action,
                        algo: *self.current_algo.read().await,
                        manual_override: false,
                        step: None,
                    },
                    None => ControlResponse::Error {
                        code: "NO_ACTION".into(),
                        message: "暂无最近动作".into(),
                    },
                }
            }
            ControlRequest::SetManualAction {
                furnace_id,
                action,
            } => {
                let mut overrides = self.manual_overrides.write().await;
                overrides.insert(furnace_id.clone(), action.clone());
                let base = match self.furnace_configs.get(&furnace_id) {
                    Some(fc) => match fc.furnace_type {
                        FurnaceType::HanChaogang => (25.0, 35.0),
                        FurnaceType::MingBlast => (32.0, 50.0),
                    },
                    _ => (25.0, 35.0),
                };
                ControlResponse::ManualOverrideApplied {
                    furnace_id,
                    action: RLAction {
                        frequency: action.frequency.unwrap_or(base.0),
                        stroke: action.stroke.unwrap_or(base.1),
                        timestamp: chrono::Utc::now(),
                        q_value: None,
                    },
                    expires_at: action.expires_at,
                }
            }
            ControlRequest::ClearManualOverride { furnace_id } => {
                let mut overrides = self.manual_overrides.write().await;
                overrides.remove(&furnace_id);
                ControlResponse::ManualOverrideCleared { furnace_id }
            }
            ControlRequest::SwitchAlgorithm { algo } => {
                let mut current = self.current_algo.write().await;
                let from = *current;
                *current = algo;
                info!("控制算法切换: {:?} -> {:?}", from, algo);
                ControlResponse::AlgoSwitched { from, to: algo }
            }
            ControlRequest::ResetController { furnace_id } => {
                let algo = *self.current_algo.read().await;
                match algo {
                    ControlAlgorithm::QLearning => {
                        let mut ql = self.ql_controller.write().await;
                        if let Some(cfg) = self.furnace_configs.get(&furnace_id) {
                            ql.add_furnace(furnace_id.clone(), cfg.clone());
                        }
                    }
                    ControlAlgorithm::Ddpg => {
                        self.ddpg_controller.reset(&furnace_id);
                    }
                }
                ControlResponse::ControllerReset {
                    furnace_id,
                    algo,
                }
            }
            ControlRequest::GetAlgo => ControlResponse::AlgoInfo {
                algo: *self.current_algo.read().await,
            },
        }
    }
}

fn build_ql_step(
    furnace_id: &str,
    action: &RLAction,
    reading: &SensorReading,
) -> RLControlStep {
    RLControlStep {
        step_id: uuid::Uuid::new_v4().to_string(),
        furnace_id: furnace_id.to_string(),
        timestamp: reading.timestamp,
        state_vector: vec![
            reading.furnace_temp,
            reading.co_concentration,
            reading.energy_efficiency,
        ],
        proposed_frequency: action.frequency,
        proposed_stroke: action.stroke,
        q_value: action.q_value.unwrap_or(0.0),
        critic_value: 0.0,
        reward: 0.0,
        epsilon: 0.0,
        episode: 0,
        algo: "q_learning".to_string(),
    }
}
