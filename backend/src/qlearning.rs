use std::collections::{HashMap, VecDeque};

use serde::{Deserialize, Serialize};
use tracing::{debug, info, warn};

use crate::models::{FurnaceConfig, FurnaceType, RLAction, SensorReading};

pub const QL_FREQ_BINS: usize = 5;
pub const QL_STROKE_BINS: usize = 5;
pub const QL_TEMP_BINS: usize = 8;
pub const QL_CO_BINS: usize = 4;
pub const QL_EFF_BINS: usize = 3;

pub const QL_LEARNING_RATE: f64 = 0.08;
pub const QL_DISCOUNT: f64 = 0.92;
pub const QL_EPSILON_START: f64 = 0.8;
pub const QL_EPSILON_MIN: f64 = 0.05;
pub const QL_EPSILON_DECAY: f64 = 0.9995;
pub const QL_REPLAY_SIZE: usize = 4000;
pub const QL_TRAIN_INTERVAL: usize = 2;

const FREQ_MIN: f64 = 10.0;
const FREQ_MAX: f64 = 60.0;
const STROKE_MIN: f64 = 15.0;
const STROKE_MAX: f64 = 80.0;

#[derive(Debug, Clone, Hash, Eq, PartialEq)]
struct DiscreteState {
    temp_bin: usize,
    co_bin: usize,
    eff_bin: usize,
}

#[derive(Debug, Clone, Copy, Hash, Eq, PartialEq)]
struct DiscreteAction {
    freq_bin: usize,
    stroke_bin: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct QLearningStatus {
    pub epsilon: f64,
    pub q_table_size: usize,
    pub replay_size: usize,
    pub episodes: usize,
    pub avg_reward: f64,
    pub last_action_freq: f64,
    pub last_action_stroke: f64,
}

#[derive(Debug, Clone)]
struct Transition {
    state: DiscreteState,
    action: DiscreteAction,
    reward: f64,
    next_state: DiscreteState,
    done: bool,
}

pub struct QLearningController {
    furnace_id: String,
    config: FurnaceConfig,
    q_table: HashMap<DiscreteState, [f64; QL_FREQ_BINS * QL_STROKE_BINS]>,
    replay_buffer: VecDeque<Transition>,
    epsilon: f64,
    episodes: usize,
    total_reward: f64,
    last_state: Option<DiscreteState>,
    last_action: Option<DiscreteAction>,
    last_freq: f64,
    last_stroke: f64,
    step_count: usize,
}

impl QLearningController {
    pub fn new(furnace_id: String, config: FurnaceConfig) -> Self {
        let (base_freq, base_stroke) = match config.furnace_type {
            FurnaceType::HanChaogang => (25.0, 35.0),
            FurnaceType::MingBlast => (32.0, 50.0),
        };
        Self {
            furnace_id,
            config,
            q_table: HashMap::new(),
            replay_buffer: VecDeque::with_capacity(QL_REPLAY_SIZE),
            epsilon: QL_EPSILON_START,
            episodes: 0,
            total_reward: 0.0,
            last_state: None,
            last_action: None,
            last_freq: base_freq,
            last_stroke: base_stroke,
            step_count: 0,
        }
    }

    fn discretize_state(&self, reading: &SensorReading) -> DiscreteState {
        let target_center =
            (self.config.target_temp_min + self.config.target_temp_max) / 2.0;
        let target_range =
            (self.config.target_temp_max - self.config.target_temp_min).max(1.0);

        let temp_deviation = (reading.furnace_temp - target_center) / target_range;
        let temp_bin = ((temp_deviation + 1.0) / 2.0 * QL_TEMP_BINS as f64)
            .round() as usize;
        let temp_bin = temp_bin.min(QL_TEMP_BINS - 1);

        let co_max = 800.0;
        let co_bin = ((reading.co_concentration / co_max) * QL_CO_BINS as f64)
            .round() as usize;
        let co_bin = co_bin.min(QL_CO_BINS - 1);

        let eff = reading.energy_efficiency.max(0.0).min(1.0);
        let eff_bin = (eff * QL_EFF_BINS as f64).floor() as usize;
        let eff_bin = eff_bin.min(QL_EFF_BINS - 1);

        DiscreteState { temp_bin, co_bin, eff_bin }
    }

    fn action_from_bin(freq_bin: usize, stroke_bin: usize) -> (f64, f64) {
        let freq = FREQ_MIN
            + (freq_bin as f64 / (QL_FREQ_BINS - 1) as f64) * (FREQ_MAX - FREQ_MIN);
        let stroke = STROKE_MIN
            + (stroke_bin as f64 / (QL_STROKE_BINS - 1) as f64)
                * (STROKE_MAX - STROKE_MIN);
        (freq.round(), stroke.round())
    }

    fn bin_index(freq_bin: usize, stroke_bin: usize) -> usize {
        freq_bin * QL_STROKE_BINS + stroke_bin
    }

    fn get_q_values(&self, state: &DiscreteState) -> [f64; QL_FREQ_BINS * QL_STROKE_BINS] {
        self.q_table.get(state).copied().unwrap_or([0.0; QL_FREQ_BINS * QL_STROKE_BINS])
    }

    fn select_best_action(&self, state: &DiscreteState) -> DiscreteAction {
        let q = self.get_q_values(state);
        let mut best_idx = 0;
        let mut best_q = f64::NEG_INFINITY;
        for (i, val) in q.iter().enumerate() {
            if *val > best_q {
                best_q = *val;
                best_idx = i;
            }
        }
        let freq_bin = best_idx / QL_STROKE_BINS;
        let stroke_bin = best_idx % QL_STROKE_BINS;
        DiscreteAction { freq_bin, stroke_bin }
    }

    fn select_random_action(&self) -> DiscreteAction {
        use rand::Rng;
        let mut rng = rand::thread_rng();
        DiscreteAction {
            freq_bin: rng.gen_range(0..QL_FREQ_BINS),
            stroke_bin: rng.gen_range(0..QL_STROKE_BINS),
        }
    }

    pub fn compute_reward(&self, reading: &SensorReading, dt: f64) -> f64 {
        let tmin = self.config.target_temp_min;
        let tmax = self.config.target_temp_max;
        let t = reading.furnace_temp;

        let temp_reward = if t >= tmin && t <= tmax {
            100.0
        } else {
            let center = (tmin + tmax) / 2.0;
            let half = (tmax - tmin) / 2.0;
            let dev = (t - center).abs() / half.max(1.0);
            100.0 - 80.0 * dev.min(2.0)
        };

        let co_penalty = if reading.co_concentration > 500.0 {
            -25.0 * ((reading.co_concentration - 500.0) / 300.0).min(2.0)
        } else {
            0.0
        };

        let eff_reward = reading.energy_efficiency * 40.0;

        temp_reward + co_penalty + eff_reward
    }

    pub fn select_action(&mut self, reading: &SensorReading) -> RLAction {
        self.step_count += 1;
        let state = self.discretize_state(reading);

        let use_random = rand::random::<f64>() < self.epsilon;
        let action = if use_random {
            self.select_random_action()
        } else {
            self.select_best_action(&state)
        };

        let (freq, stroke) = Self::action_from_bin(action.freq_bin, action.stroke_bin);

        let (safe_freq, safe_stroke) = self.apply_safety_constraints(freq, stroke, reading);

        if let Some(prev_state) = self.last_state.take() {
            if let Some(prev_action) = self.last_action.take() {
                let reward = self.compute_reward(reading, 10.0);
                self.total_reward += reward;
                let transition = Transition {
                    state: prev_state,
                    action: prev_action,
                    reward,
                    next_state: state.clone(),
                    done: false,
                };
                self.replay_buffer.push_back(transition);
                if self.replay_buffer.len() > QL_REPLAY_SIZE {
                    self.replay_buffer.pop_front();
                }

                if self.step_count % QL_TRAIN_INTERVAL == 0 {
                    self.train_from_replay();
                }

                self.epsilon = (self.epsilon * QL_EPSILON_DECAY).max(QL_EPSILON_MIN);
                self.episodes += 1;
            }
        }

        self.last_state = Some(state);
        self.last_action = Some(action);
        self.last_freq = safe_freq;
        self.last_stroke = safe_stroke;

        RLAction {
            frequency: safe_freq,
            stroke: safe_stroke,
            timestamp: reading.timestamp,
            q_value: Some(self.estimate_q()),
        }
    }

    fn estimate_q(&self) -> f64 {
        if let Some(state) = &self.last_state {
            let q = self.get_q_values(state);
            let best = q.iter().cloned().fold(f64::NEG_INFINITY, f64::max);
            best
        } else {
            0.0
        }
    }

    fn train_from_replay(&mut self) {
        if self.replay_buffer.len() < 16 {
            return;
        }
        let batch: Vec<&Transition> = self.replay_buffer.iter().take(32).collect();

        for trans in batch.iter() {
            let next_q = self
                .q_table
                .get(&trans.next_state)
                .map(|q| q.iter().cloned().fold(f64::NEG_INFINITY, f64::max))
                .unwrap_or(0.0);

            let target = trans.reward + QL_DISCOUNT * next_q * (1.0 - trans.done as u8 as f64);

            let q_entry = self
                .q_table
                .entry(trans.state.clone())
                .or_insert([0.0; QL_FREQ_BINS * QL_STROKE_BINS]);

            let idx = Self::bin_index(trans.action.freq_bin, trans.action.stroke_bin);
            let old_q = q_entry[idx];
            q_entry[idx] += QL_LEARNING_RATE * (target - old_q);
        }
    }

    fn apply_safety_constraints(
        &self,
        freq: f64,
        stroke: f64,
        reading: &SensorReading,
    ) -> (f64, f64) {
        let mut f = freq.clamp(FREQ_MIN, FREQ_MAX);
        let mut s = stroke.clamp(STROKE_MIN, STROKE_MAX);

        let max_delta_f = 8.0;
        let max_delta_s = 10.0;

        f = (f - self.last_freq).clamp(-max_delta_f, max_delta_f) + self.last_freq;
        s = (s - self.last_stroke).clamp(-max_delta_s, max_delta_s) + self.last_stroke;

        if reading.furnace_temp > self.config.target_temp_max + 120.0 {
            f = (self.last_freq - 12.0).max(FREQ_MIN);
            s = (self.last_stroke - 15.0).max(STROKE_MIN);
            warn!("[QL-{}] 紧急减风：温度{}过高", self.furnace_id, reading.furnace_temp);
        }

        if reading.furnace_temp < self.config.target_temp_min - 150.0 {
            f = (self.last_freq + 10.0).min(FREQ_MAX);
            s = (self.last_stroke + 12.0).min(STROKE_MAX);
            warn!("[QL-{}] 紧急加风：温度{}过低", self.furnace_id, reading.furnace_temp);
        }

        if reading.co_concentration > 700.0 {
            f = (self.last_freq * 1.15).min(FREQ_MAX);
        }

        (f, s)
    }

    pub fn get_status(&self) -> QLearningStatus {
        QLearningStatus {
            epsilon: self.epsilon,
            q_table_size: self.q_table.len(),
            replay_size: self.replay_buffer.len(),
            episodes: self.episodes,
            avg_reward: if self.episodes > 0 {
                self.total_reward / self.episodes as f64
            } else {
                0.0
            },
            last_action_freq: self.last_freq,
            last_action_stroke: self.last_stroke,
        }
    }

    pub fn reset(&mut self) {
        self.q_table.clear();
        self.replay_buffer.clear();
        self.epsilon = QL_EPSILON_START;
        self.episodes = 0;
        self.total_reward = 0.0;
        self.last_state = None;
        self.last_action = None;
        self.step_count = 0;
    }
}

pub struct MultiFurnaceQLController {
    controllers: HashMap<String, QLearningController>,
}

impl MultiFurnaceQLController {
    pub fn new() -> Self {
        Self { controllers: HashMap::new() }
    }

    pub fn add_furnace(&mut self, furnace_id: String, config: FurnaceConfig) {
        let ctrl = QLearningController::new(furnace_id.clone(), config);
        self.controllers.insert(furnace_id, ctrl);
    }

    pub fn select_action(
        &mut self,
        furnace_id: &str,
        reading: &SensorReading,
    ) -> Option<RLAction> {
        self.controllers
            .get_mut(furnace_id)
            .map(|c| c.select_action(reading))
    }

    pub fn get_status(&self, furnace_id: &str) -> Option<QLearningStatus> {
        self.controllers.get(furnace_id).map(|c| c.get_status())
    }

    pub fn all_statuses(&self) -> Vec<(String, QLearningStatus)> {
        self.controllers
            .iter()
            .map(|(k, v)| (k.clone(), v.get_status()))
            .collect()
    }
}

impl Default for MultiFurnaceQLController {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_discretization_bounds() {
        let config = FurnaceConfig {
            id: "T1".into(),
            name: "".into(),
            furnace_type: FurnaceType::HanChaogang,
            volume_m3: 2.5,
            target_temp_min: 1200.0,
            target_temp_max: 1350.0,
        };
        let ctrl = QLearningController::new("T1".into(), config);

        let reading = SensorReading::mock("T1");
        let s = ctrl.discretize_state(&reading);

        assert!(s.temp_bin < QL_TEMP_BINS);
        assert!(s.co_bin < QL_CO_BINS);
        assert!(s.eff_bin < QL_EFF_BINS);
    }

    #[test]
    fn test_action_mapping() {
        for f in 0..QL_FREQ_BINS {
            for s in 0..QL_STROKE_BINS {
                let (freq, stroke) = QLearningController::action_from_bin(f, s);
                assert!(freq >= FREQ_MIN && freq <= FREQ_MAX);
                assert!(stroke >= STROKE_MIN && stroke <= STROKE_MAX);
            }
        }
    }
}
