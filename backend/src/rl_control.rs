use std::collections::VecDeque;

use chrono::{DateTime, Utc};
use parking_lot::RwLock;
use rand::Rng;
use rand_distr::{Normal, Distribution};
use tracing::{debug, info};

use crate::models::{
    ControlStep, FurnaceConfig, RLAction, RLState, SensorReading,
};

pub const STATE_DIM: usize = 10;
pub const ACTION_DIM: usize = 2;
pub const REPLAY_BUFFER_SIZE: usize = 20000;
pub const BATCH_SIZE: usize = 64;
pub const GAMMA: f64 = 0.99;
pub const TAU: f64 = 0.005;
pub const ACTOR_LR: f64 = 0.0001;
pub const CRITIC_LR: f64 = 0.0003;
pub const DEFAULT_EPSILON: f64 = 0.9;
pub const EPSILON_DECAY: f64 = 0.9995;
pub const MIN_EPSILON: f64 = 0.05;

pub const FREQ_MIN: f64 = 10.0;
pub const FREQ_MAX: f64 = 60.0;
pub const STROKE_MIN: f64 = 15.0;
pub const STROKE_MAX: f64 = 80.0;

pub struct ReplayBuffer {
    buffer: VecDeque<Experience>,
    max_size: usize,
}

#[derive(Debug, Clone)]
pub struct Experience {
    pub state: Vec<f64>,
    pub action: (f64, f64),
    pub reward: f64,
    pub next_state: Vec<f64>,
    pub done: bool,
}

impl ReplayBuffer {
    pub fn new(max_size: usize) -> Self {
        Self {
            buffer: VecDeque::with_capacity(max_size),
            max_size,
        }
    }

    pub fn push(&mut self, exp: Experience) {
        if self.buffer.len() >= self.max_size {
            self.buffer.pop_front();
        }
        self.buffer.push_back(exp);
    }

    pub fn sample(&self, batch_size: usize) -> Vec<&Experience> {
        let mut rng = rand::thread_rng();
        let n = self.buffer.len().min(batch_size);
        (0..n)
            .map(|_| {
                let idx = rng.gen_range(0..self.buffer.len());
                &self.buffer[idx]
            })
            .collect()
    }

    pub fn len(&self) -> usize {
        self.buffer.len()
    }

    pub fn is_empty(&self) -> bool {
        self.buffer.is_empty()
    }
}

#[derive(Clone)]
struct ActorNetwork {
    weights_input_hidden: Vec<Vec<f64>>,
    weights_hidden_output: Vec<Vec<f64>>,
    bias_hidden: Vec<f64>,
    bias_output: Vec<f64>,
    hidden_size: usize,
}

impl ActorNetwork {
    fn new(input_dim: usize, hidden_size: usize, output_dim: usize) -> Self {
        let mut rng = rand::thread_rng();
        let normal = Normal::new(0.0, 0.1).unwrap();

        let weights_input_hidden = (0..input_dim)
            .map(|_| (0..hidden_size).map(|_| normal.sample(&mut rng)).collect())
            .collect();

        let weights_hidden_output = (0..hidden_size)
            .map(|_| (0..output_dim).map(|_| normal.sample(&mut rng)).collect())
            .collect();

        let bias_hidden = (0..hidden_size).map(|_| 0.01).collect();
        let bias_output = (0..output_dim).map(|_| 0.0).collect();

        Self {
            weights_input_hidden,
            weights_hidden_output,
            bias_hidden,
            bias_output,
            hidden_size,
        }
    }

    fn forward(&self, state: &[f64]) -> Vec<f64> {
        let mut hidden = vec![0.0; self.hidden_size];

        for j in 0..self.hidden_size {
            let mut sum = self.bias_hidden[j];
            for (i, &s) in state.iter().enumerate() {
                sum += s * self.weights_input_hidden[i][j];
            }
            hidden[j] = sum.tanh();
        }

        let mut output = vec![0.0; self.weights_hidden_output[0].len()];
        for k in 0..output.len() {
            let mut sum = self.bias_output[k];
            for (j, &h) in hidden.iter().enumerate() {
                sum += h * self.weights_hidden_output[j][k];
            }
            output[k] = sum.tanh();
        }

        output
    }

    fn predict_action(&self, state: &[f64]) -> RLAction {
        let raw = self.forward(state);
        RLAction {
            frequency: scale_action(raw[0], FREQ_MIN, FREQ_MAX),
            stroke: scale_action(raw[1], STROKE_MIN, STROKE_MAX),
        }
    }
}

#[derive(Clone)]
struct CriticNetwork {
    weights_s_hidden: Vec<Vec<f64>>,
    weights_a_hidden: Vec<Vec<f64>>,
    weights_hidden_output: Vec<f64>,
    bias_hidden: Vec<f64>,
    bias_output: f64,
    state_dim: usize,
    action_dim: usize,
    hidden_size: usize,
}

impl CriticNetwork {
    fn new(state_dim: usize, action_dim: usize, hidden_size: usize) -> Self {
        let mut rng = rand::thread_rng();
        let normal = Normal::new(0.0, 0.1).unwrap();

        let weights_s_hidden = (0..state_dim)
            .map(|_| (0..hidden_size).map(|_| normal.sample(&mut rng)).collect())
            .collect();

        let weights_a_hidden = (0..action_dim)
            .map(|_| (0..hidden_size).map(|_| normal.sample(&mut rng)).collect())
            .collect();

        let weights_hidden_output = (0..hidden_size).map(|_| normal.sample(&mut rng)).collect();
        let bias_hidden = (0..hidden_size).map(|_| 0.01).collect();

        Self {
            weights_s_hidden,
            weights_a_hidden,
            weights_hidden_output,
            bias_hidden,
            bias_output: 0.0,
            state_dim,
            action_dim,
            hidden_size,
        }
    }

    fn forward(&self, state: &[f64], action: &[f64]) -> f64 {
        let mut hidden = vec![0.0; self.hidden_size];

        for j in 0..self.hidden_size {
            let mut sum = self.bias_hidden[j];
            for (i, &s) in state.iter().enumerate() {
                sum += s * self.weights_s_hidden[i][j];
            }
            for (i, &a) in action.iter().enumerate() {
                sum += a * self.weights_a_hidden[i][j];
            }
            hidden[j] = sum.tanh();
        }

        let mut q = self.bias_output;
        for (j, &h) in hidden.iter().enumerate() {
            q += h * self.weights_hidden_output[j];
        }
        q
    }
}

fn scale_action(x: f64, min: f64, max: f64) -> f64 {
    let x_clamped = x.clamp(-1.0, 1.0);
    min + (x_clamped + 1.0) * 0.5 * (max - min)
}

fn unscale_action(x: f64, min: f64, max: f64) -> f64 {
    ((x - min) / (max - min)) * 2.0 - 1.0
}

fn normalize_state(state: &RLState) -> Vec<f64> {
    vec![
        (state.furnace_temp - 1000.0) / 800.0,
        state.temp_deviation / 300.0,
        state.co_concentration / 8.0,
        (state.wind_pressure - 1000.0) / 2000.0,
        state.air_volume / 2.0,
        state.energy_efficiency / 100.0,
        (state.current_frequency - 35.0) / 30.0,
        (state.current_stroke - 45.0) / 40.0,
        (state.reaction_rate - 0.5) / 1.0,
        state.temp_gradient / 50.0,
    ]
}

pub fn build_rl_state(
    reading: &SensorReading,
    config: &FurnaceConfig,
    prev_temp: f64,
) -> RLState {
    let target_center = (config.target_temp_min + config.target_temp_max) / 2.0;
    let temp_deviation = reading.furnace_temp - target_center;
    let temp_gradient = reading.furnace_temp - prev_temp;
    let temp_zones = reading.temp_zones();

    RLState {
        furnace_temp: reading.furnace_temp,
        temp_deviation,
        co_concentration: reading.co_concentration,
        wind_pressure: reading.wind_pressure,
        air_volume: reading.air_volume,
        energy_efficiency: reading.energy_efficiency,
        current_frequency: reading.push_pull_frequency,
        current_stroke: reading.stroke_length,
        reaction_rate: reading.reaction_rate,
        temp_gradient,
    }
}

pub fn compute_reward(
    state: &RLState,
    config: &FurnaceConfig,
) -> f64 {
    let mut reward = 0.0;

    let temp = state.furnace_temp;
    let t_min = config.target_temp_min;
    let t_max = config.target_temp_max;
    let t_center = (t_min + t_max) / 2.0;

    if temp >= t_min && temp <= t_max {
        let margin = (t_max - t_min) * 0.2;
        if temp >= t_min + margin && temp <= t_max - margin {
            reward += 100.0;
        } else {
            reward += 70.0;
        }
        let dist_to_center = (temp - t_center).abs() / ((t_max - t_min) * 0.5);
        reward += 30.0 * (1.0 - dist_to_center);
    } else {
        let overflow = if temp > t_max { temp - t_max } else { t_min - temp };
        reward -= overflow.powi(2) * 0.05;
    }

    if state.co_concentration > 3.0 {
        reward -= (state.co_concentration - 3.0) * 25.0;
    } else if state.co_concentration > 5.0 {
        reward -= (state.co_concentration - 5.0) * 80.0;
    }

    reward += state.energy_efficiency * 0.5;

    let freq_opt = (state.current_frequency - 35.0).abs();
    let stroke_opt = (state.current_stroke - 45.0).abs();
    reward -= (freq_opt * 0.3 + stroke_opt * 0.2);

    let grad_penalty = state.temp_gradient.abs() * 2.0;
    reward -= grad_penalty;

    reward.clamp(-200.0, 200.0)
}

pub struct RLTrainer {
    furnace_id: String,
    actor: ActorNetwork,
    actor_target: ActorNetwork,
    critic: CriticNetwork,
    critic_target: CriticNetwork,
    replay_buffer: ReplayBuffer,
    epsilon: f64,
    pub episode: u32,
    pub step: u32,
    pub last_reward: f64,
    pub last_loss: f64,
    prev_state: Option<Vec<f64>>,
    prev_action: Option<(f64, f64)>,
    total_reward: f64,
    steps_per_episode: u32,
}

impl RLTrainer {
    pub fn new(furnace_id: String) -> Self {
        let hidden_size = 128;
        let actor = ActorNetwork::new(STATE_DIM, hidden_size, ACTION_DIM);
        let actor_target = actor.clone();
        let critic = CriticNetwork::new(STATE_DIM, ACTION_DIM, hidden_size);
        let critic_target = critic.clone();

        Self {
            furnace_id,
            actor,
            actor_target,
            critic,
            critic_target,
            replay_buffer: ReplayBuffer::new(REPLAY_BUFFER_SIZE),
            epsilon: DEFAULT_EPSILON,
            episode: 0,
            step: 0,
            last_reward: 0.0,
            last_loss: 0.0,
            prev_state: None,
            prev_action: None,
            total_reward: 0.0,
            steps_per_episode: 360,
        }
    }

    pub fn select_action(&mut self, state: &RLState) -> RLAction {
        let norm_state = normalize_state(state);

        let mut rng = rand::thread_rng();
        if rng.gen::<f64>() < self.epsilon {
            RLAction {
                frequency: rng.gen_range(FREQ_MIN..FREQ_MAX),
                stroke: rng.gen_range(STROKE_MIN..STROKE_MAX),
            }
        } else {
            let mut action = self.actor.predict_action(&norm_state);
            let noise_scale = self.epsilon * 0.15;
            action.frequency += rng.gen_range(-noise_scale..noise_scale) * (FREQ_MAX - FREQ_MIN);
            action.stroke += rng.gen_range(-noise_scale..noise_scale) * (STROKE_MAX - STROKE_MIN);
            action.frequency = action.frequency.clamp(FREQ_MIN, FREQ_MAX);
            action.stroke = action.stroke.clamp(STROKE_MIN, STROKE_MAX);
            action
        }
    }

    pub fn select_safe_action(
        &mut self,
        state: &RLState,
        config: &FurnaceConfig,
    ) -> (RLAction, Option<ControlStep>) {
        let norm_state = normalize_state(state);
        let action = self.select_action(state);

        let (scaled_freq, scaled_stroke) = self.apply_safety_constraints(
            action.frequency,
            action.stroke,
            state,
            config,
        );

        let safe_action = RLAction {
            frequency: scaled_freq,
            stroke: scaled_stroke,
        };

        let mut control_step = None;

        if let (Some(prev_s), Some(prev_a)) = (self.prev_state.take(), self.prev_action.take()) {
            let reward = compute_reward(state, config);
            self.last_reward = reward;
            self.total_reward += reward;
            self.step += 1;

            let done = (self.step % self.steps_per_episode == 0) as u8;
            if done == 1 {
                self.episode += 1;
                self.epsilon = (self.epsilon * EPSILON_DECAY).max(MIN_EPSILON);
                info!(
                    "[RL-{}] Episode {} 完成: 累计奖励={:.1}, ε={:.3}",
                    self.furnace_id, self.episode, self.total_reward, self.epsilon
                );
                self.total_reward = 0.0;
            }

            let next_norm_state = norm_state.clone();

            self.replay_buffer.push(Experience {
                state: prev_s.clone(),
                action: prev_a,
                reward,
                next_state: next_norm_state.clone(),
                done: done == 1,
            });

            let loss = self.train_step();
            self.last_loss = loss;

            control_step = Some(ControlStep {
                timestamp: Utc::now(),
                furnace_id: self.furnace_id.clone(),
                episode: self.episode,
                step: self.step,
                state_vector: prev_s,
                action_frequency: prev_a.0,
                action_stroke: prev_a.1,
                reward,
                next_state_vector: next_norm_state,
                done,
                loss,
                epsilon: self.epsilon,
                learning_rate: ACTOR_LR,
            });
        }

        self.prev_state = Some(norm_state);
        self.prev_action = Some((safe_action.frequency, safe_action.stroke));

        (safe_action, control_step)
    }

    fn apply_safety_constraints(
        &self,
        freq: f64,
        stroke: f64,
        state: &RLState,
        config: &FurnaceConfig,
    ) -> (f64, f64) {
        let mut f = freq.clamp(FREQ_MIN, FREQ_MAX);
        let mut s = stroke.clamp(STROKE_MIN, STROKE_MAX);

        let temp = state.furnace_temp;
        let co = state.co_concentration;
        let trend = state.temp_gradient;

        if temp > config.target_temp_max + 50.0 && trend > 0.0 {
            f = f.min(state.current_frequency * 0.8);
            s = s.min(state.current_stroke * 0.85);
        } else if temp > config.target_temp_max + 100.0 {
            f = FREQ_MIN;
            s = STROKE_MIN * 1.1;
        }

        if temp < config.target_temp_min - 50.0 && trend < 0.0 {
            f = f.max(state.current_frequency * 1.15);
            s = s.max(state.current_stroke * 1.1);
        } else if temp < config.target_temp_min - 150.0 {
            f = FREQ_MAX * 0.85;
            s = STROKE_MAX * 0.7;
        }

        if co > 5.0 {
            f = f.max(state.current_frequency * 1.2);
        } else if co > 3.0 {
            f = f.max(state.current_frequency * 1.05);
        }

        let max_change_freq = (FREQ_MAX - FREQ_MIN) * 0.15;
        let max_change_stroke = (STROKE_MAX - STROKE_MIN) * 0.15;
        f = f.clamp(
            state.current_frequency - max_change_freq,
            state.current_frequency + max_change_freq,
        );
        s = s.clamp(
            state.current_stroke - max_change_stroke,
            state.current_stroke + max_change_stroke,
        );

        (f.clamp(FREQ_MIN, FREQ_MAX), s.clamp(STROKE_MIN, STROKE_MAX))
    }

    fn train_step(&mut self) -> f64 {
        if self.replay_buffer.len() < BATCH_SIZE {
            return 0.0;
        }

        let batch = self.replay_buffer.sample(BATCH_SIZE);
        let mut total_critic_loss = 0.0;
        let mut rng = rand::thread_rng();

        for exp in &batch {
            let raw_action = [
                unscale_action(exp.action.0, FREQ_MIN, FREQ_MAX),
                unscale_action(exp.action.1, STROKE_MIN, STROKE_MAX),
            ];

            let next_raw_action = self.actor_target.forward(&exp.next_state);
            let target_q = if exp.done {
                exp.reward
            } else {
                let next_q = self.critic_target.forward(&exp.next_state, &next_raw_action);
                exp.reward + GAMMA * next_q
            };

            let current_q = self.critic.forward(&exp.state, &raw_action);
            let td_error = target_q - current_q;
            total_critic_loss += td_error.powi(2);

            let lr_c = CRITIC_LR;
            let mut s_grad = vec![0.0; STATE_DIM];
            let mut a_grad = vec![0.0; ACTION_DIM];

            for j in 0..self.critic.hidden_size {
                let pre_activation = self.critic.bias_hidden[j]
                    + exp
                        .state
                        .iter()
                        .enumerate()
                        .map(|(i, s)| s * self.critic.weights_s_hidden[i][j])
                        .sum::<f64>()
                    + raw_action
                        .iter()
                        .enumerate()
                        .map(|(i, a)| a * self.critic.weights_a_hidden[i][j])
                        .sum::<f64>();

                let d_activation = 1.0 - pre_activation.tanh().powi(2);
                let d_loss = -2.0 * td_error * d_activation * self.critic.weights_hidden_output[j];

                for i in 0..STATE_DIM {
                    self.critic.weights_s_hidden[i][j] -= lr_c * d_loss * exp.state[i];
                    s_grad[i] += d_loss * self.critic.weights_s_hidden[i][j];
                }
                for i in 0..ACTION_DIM {
                    self.critic.weights_a_hidden[i][j] -= lr_c * d_loss * raw_action[i];
                    a_grad[i] += d_loss * self.critic.weights_a_hidden[i][j];
                }
                self.critic.bias_hidden[j] -= lr_c * d_loss;
            }

            for j in 0..self.critic.hidden_size {
                let pre_activation = self.critic.bias_hidden[j]
                    + exp
                        .state
                        .iter()
                        .enumerate()
                        .map(|(i, s)| s * self.critic.weights_s_hidden[i][j])
                        .sum::<f64>()
                    + raw_action
                        .iter()
                        .enumerate()
                        .map(|(i, a)| a * self.critic.weights_a_hidden[i][j])
                        .sum::<f64>();

                let h = pre_activation.tanh();
                self.critic.weights_hidden_output[j] -= lr_c * (-2.0 * td_error) * h;
            }
            self.critic.bias_output -= lr_c * (-2.0 * td_error);

            let lr_a = ACTOR_LR;
            let actor_output = self.actor.forward(&exp.state);
            let mut critic_input_grad = vec![0.0; ACTION_DIM];
            for k in 0..ACTION_DIM {
                critic_input_grad[k] = a_grad[k].signum() * 0.1;
            }

            for k in 0..ACTION_DIM {
                let d_output = critic_input_grad[k] * (1.0 - actor_output[k].powi(2));
                for j in 0..self.actor.hidden_size {
                    let h_input = self.actor.bias_hidden[j]
                        + exp
                            .state
                            .iter()
                            .enumerate()
                            .map(|(i, s)| s * self.actor.weights_input_hidden[i][j])
                            .sum::<f64>();
                    let h = h_input.tanh();
                    let d_h = d_output * (1.0 - h.powi(2)) * self.actor.weights_hidden_output[j][k];

                    for i in 0..STATE_DIM {
                        self.actor.weights_input_hidden[i][j] -= lr_a * d_h * exp.state[i];
                    }
                    self.actor.bias_hidden[j] -= lr_a * d_h;
                    self.actor.weights_hidden_output[j][k] -= lr_a * d_output * h;
                }
                self.actor.bias_output[k] -= lr_a * d_output;
            }
        }

        self.update_target_networks();
        total_critic_loss / batch.len() as f64
    }

    fn update_target_networks(&mut self) {
        for i in 0..STATE_DIM {
            for j in 0..self.actor.hidden_size {
                self.actor_target.weights_input_hidden[i][j] =
                    (1.0 - TAU) * self.actor_target.weights_input_hidden[i][j]
                        + TAU * self.actor.weights_input_hidden[i][j];
            }
        }
        for j in 0..self.actor.hidden_size {
            self.actor_target.bias_hidden[j] =
                (1.0 - TAU) * self.actor_target.bias_hidden[j] + TAU * self.actor.bias_hidden[j];
            for k in 0..ACTION_DIM {
                self.actor_target.weights_hidden_output[j][k] =
                    (1.0 - TAU) * self.actor_target.weights_hidden_output[j][k]
                        + TAU * self.actor.weights_hidden_output[j][k];
            }
        }
        for k in 0..ACTION_DIM {
            self.actor_target.bias_output[k] =
                (1.0 - TAU) * self.actor_target.bias_output[k] + TAU * self.actor.bias_output[k];
        }
    }

    pub fn get_status(&self) -> RLStatus {
        RLStatus {
            furnace_id: self.furnace_id.clone(),
            episode: self.episode,
            step: self.step,
            epsilon: self.epsilon,
            last_reward: self.last_reward,
            last_loss: self.last_loss,
            buffer_size: self.replay_buffer.len(),
        }
    }
}

#[derive(Debug, Clone, serde::Serialize)]
pub struct RLStatus {
    pub furnace_id: String,
    pub episode: u32,
    pub step: u32,
    pub epsilon: f64,
    pub last_reward: f64,
    pub last_loss: f64,
    pub buffer_size: usize,
}

pub struct MultiFurnaceRLController {
    trainers: std::collections::HashMap<String, RwLock<RLTrainer>>,
}

impl MultiFurnaceRLController {
    pub fn new() -> Self {
        Self {
            trainers: std::collections::HashMap::new(),
        }
    }

    pub fn add_furnace(&mut self, furnace_id: String) {
        let trainer = RLTrainer::new(furnace_id.clone());
        self.trainers.insert(furnace_id, RwLock::new(trainer));
    }

    pub fn process_reading(
        &self,
        reading: &SensorReading,
        config: &FurnaceConfig,
        prev_temp: f64,
    ) -> (RLAction, Option<ControlStep>) {
        let state = build_rl_state(reading, config, prev_temp);

        if let Some(trainer) = self.trainers.get(&reading.furnace_id) {
            let mut t = trainer.write();
            t.select_safe_action(&state, config)
        } else {
            (
                RLAction {
                    frequency: reading.push_pull_frequency,
                    stroke: reading.stroke_length,
                },
                None,
            )
        }
    }

    pub fn get_trainer_status(&self, furnace_id: &str) -> Option<RLStatus> {
        self.trainers.get(furnace_id).map(|t| t.read().get_status())
    }

    pub fn get_all_status(&self) -> Vec<RLStatus> {
        self.trainers
            .values()
            .map(|t| t.read().get_status())
            .collect()
    }
}

impl Default for MultiFurnaceRLController {
    fn default() -> Self {
        Self::new()
    }
}
