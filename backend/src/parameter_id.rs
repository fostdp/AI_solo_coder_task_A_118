use std::collections::VecDeque;

use serde::{Deserialize, Serialize};
use tracing::{debug, info, warn};

use crate::models::SensorReading;

pub const RLS_WINDOW_SIZE: usize = 50;
pub const RLS_FORGETTING_FACTOR: f64 = 0.995;
pub const RLS_INIT_P_COV: f64 = 10_000.0;
pub const RLS_MIN_SAMPLES: usize = 10;
pub const RLS_UPDATE_INTERVAL: usize = 3;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct IdentifiedParams {
    pub activation_energy: f64,
    pub pre_exponential_factor: f64,
    pub heat_loss_coefficient: f64,
    pub heat_transfer_coeff: f64,
    pub confidence: f64,
    pub sample_count: usize,
    pub residuals_mse: f64,
}

impl Default for IdentifiedParams {
    fn default() -> Self {
        Self {
            activation_energy: 160_000.0,
            pre_exponential_factor: 5.0e8,
            heat_loss_coefficient: 0.015,
            heat_transfer_coeff: 45.0,
            confidence: 0.0,
            sample_count: 0,
            residuals_mse: f64::MAX,
        }
    }
}

#[derive(Debug, Clone)]
struct RLSState {
    theta: Vec<f64>,
    p_matrix: Vec<Vec<f64>>,
    lambda: f64,
    n_params: usize,
}

impl RLSState {
    fn new(n_params: usize, lambda: f64, init_p: f64) -> Self {
        let theta = vec![0.0; n_params];
        let mut p_matrix = vec![vec![0.0; n_params]; n_params];
        for i in 0..n_params {
            p_matrix[i][i] = init_p;
        }
        Self { theta, p_matrix, lambda, n_params }
    }

    fn update(&mut self, phi: &[f64], y: f64) -> f64 {
        let n = self.n_params;

        let mut phi_p = vec![0.0; n];
        for j in 0..n {
            for k in 0..n {
                phi_p[j] += phi[k] * self.p_matrix[k][j];
            }
        }

        let mut denominator = self.lambda;
        for j in 0..n {
            denominator += phi[j] * phi_p[j];
        }

        if denominator.abs() < 1e-10 {
            return 0.0;
        }

        let gain: Vec<f64> = phi_p.iter().map(|v| v / denominator).collect();

        let prediction: f64 = phi.iter().zip(self.theta.iter()).map(|(p, t)| p * t).sum();
        let error = y - prediction;

        for i in 0..n {
            self.theta[i] += gain[i] * error;
        }

        let mut phi_gain_p = vec![vec![0.0; n]; n];
        for i in 0..n {
            for j in 0..n {
                phi_gain_p[i][j] = gain[i] * phi_p[j];
            }
        }

        for i in 0..n {
            for j in 0..n {
                self.p_matrix[i][j] = (self.p_matrix[i][j] - phi_gain_p[i][j]) / self.lambda;
            }
        }

        error
    }
}

#[derive(Debug, Clone)]
struct Observation {
    temp: f64,
    o2: f64,
    coal: f64,
    air_volume: f64,
    delta_temp: f64,
    actual_reaction_rate: f64,
    mass_load: f64,
    specific_heat: f64,
}

pub struct OnlineParameterIdentifier {
    furnace_id: String,
    rls_arrhenius: RLSState,
    rls_heat_loss: RLSState,
    observations: VecDeque<Observation>,
    last_reading: Option<SensorReading>,
    last_reaction_rate: f64,
    identified: IdentifiedParams,
    step_count: usize,
    residuals: VecDeque<f64>,
    default_ea: f64,
    default_a: f64,
    default_hlc: f64,
}

impl OnlineParameterIdentifier {
    pub fn new(
        furnace_id: String,
        default_ea: f64,
        default_a: f64,
        default_hlc: f64,
    ) -> Self {
        Self {
            furnace_id,
            rls_arrhenius: RLSState::new(2, RLS_FORGETTING_FACTOR, RLS_INIT_P_COV),
            rls_heat_loss: RLSState::new(1, RLS_FORGETTING_FACTOR, RLS_INIT_P_COV),
            observations: VecDeque::with_capacity(RLS_WINDOW_SIZE),
            last_reading: None,
            last_reaction_rate: 0.0,
            identified: IdentifiedParams::default(),
            step_count: 0,
            residuals: VecDeque::with_capacity(RLS_WINDOW_SIZE),
            default_ea,
            default_a,
            default_hlc,
        }
    }

    pub fn process_reading(&mut self, reading: &SensorReading, dt: f64) -> IdentifiedParams {
        self.step_count += 1;

        let result = if let Some(prev) = &self.last_reading {
            let delta_temp = reading.furnace_temp - prev.furnace_temp;

            let obs = Observation {
                temp: reading.furnace_temp,
                o2: reading.o2_concentration,
                coal: reading.coal_feed_rate,
                air_volume: reading.air_volume,
                delta_temp,
                actual_reaction_rate: reading.reaction_rate.max(1e-6),
                mass_load: reading.iron_feed_rate * 1000.0,
                specific_heat: 650.0,
            };

            self.observations.push_back(obs.clone());
            if self.observations.len() > RLS_WINDOW_SIZE {
                self.observations.pop_front();
            }

            if self.step_count % RLS_UPDATE_INTERVAL == 0
                && self.observations.len() >= RLS_MIN_SAMPLES
            {
                self._run_identification(dt);
            }

            self.last_reaction_rate = reading.reaction_rate;
            self.identified.clone()
        } else {
            self.identified.clone()
        };

        self.last_reading = Some(reading.clone());
        result
    }

    fn _run_identification(&mut self, dt: f64) {
        let mut total_residual = 0.0;
        let mut count = 0;

        for obs in self.observations.iter() {
            let temp_k = obs.temp + 273.15;
            let inv_rt = -1.0 / (8.314 * temp_k);

            let phi_a = [inv_rt, (obs.o2 / 21.0).max(0.1).ln().max(-10.0)];
            let y_a = obs.actual_reaction_rate.max(1e-8).ln();

            let err_a = self.rls_arrhenius.update(&phi_a, y_a);
            total_residual += err_a * err_a;

            let surface_area = 6.0 * 2.5_f64.powf(2.0 / 3.0);
            let delta_t = (obs.temp - 25.0).max(0.0);
            let phi_hl = [surface_area * delta_t * dt];
            let y_hl = obs.delta_temp * obs.mass_load * obs.specific_heat;

            let _err_hl = self.rls_heat_loss.update(&phi_hl, y_hl);
            count += 1;
        }

        if count > 0 {
            let mse = total_residual / count as f64;
            self.residuals.push_back(mse);
            if self.residuals.len() > RLS_WINDOW_SIZE {
                self.residuals.pop_front();
            }

            let theta = &self.rls_arrhenius.theta;
            let ea_est = (-theta[0]).max(80_000.0).min(250_000.0);
            let a_est = theta[1].exp().max(1e6).min(1e12);

            let ea_weight = if self.identified.sample_count < 30 { 0.3 } else { 0.9 };
            let a_weight = ea_weight;

            let ea = ea_est * ea_weight + self.default_ea * (1.0 - ea_weight);
            let a = a_est * a_weight + self.default_a * (1.0 - a_weight);

            let hlc = if !self.rls_heat_loss.theta.is_empty() {
                self.rls_heat_loss.theta[0].max(0.005).min(0.05)
            } else {
                self.default_hlc
            };
            let hlc = hlc * 0.7 + self.default_hlc * 0.3;

            let avg_mse: f64 =
                self.residuals.iter().sum::<f64>() / self.residuals.len().max(1) as f64;
            let confidence = (1.0 / (1.0 + avg_mse.sqrt())).min(1.0).max(0.0);

            self.identified = IdentifiedParams {
                activation_energy: ea,
                pre_exponential_factor: a,
                heat_loss_coefficient: hlc,
                heat_transfer_coeff: 45.0,
                confidence,
                sample_count: self.observations.len(),
                residuals_mse: avg_mse,
            };

            debug!(
                "[ParamID-{}] Ea={:.0} J/mol, A={:.2e}, hlc={:.4}, conf={:.3}, MSE={:.6}",
                self.furnace_id, ea, a, hlc, confidence, avg_mse
            );
        }
    }

    pub fn get_params(&self) -> IdentifiedParams {
        self.identified.clone()
    }

    pub fn is_stable(&self) -> bool {
        self.identified.confidence > 0.4 && self.identified.sample_count >= RLS_MIN_SAMPLES
    }

    pub fn reset(&mut self) {
        self.rls_arrhenius = RLSState::new(2, RLS_FORGETTING_FACTOR, RLS_INIT_P_COV);
        self.rls_heat_loss = RLSState::new(1, RLS_FORGETTING_FACTOR, RLS_INIT_P_COV);
        self.observations.clear();
        self.residuals.clear();
        self.last_reading = None;
        self.step_count = 0;
        self.identified = IdentifiedParams::default();
    }
}

pub struct MultiFurnaceIdentifier {
    identifiers: std::collections::HashMap<String, OnlineParameterIdentifier>,
}

impl MultiFurnaceIdentifier {
    pub fn new() -> Self {
        Self { identifiers: std::collections::HashMap::new() }
    }

    pub fn add_furnace(&mut self, furnace_id: String, defaults: (f64, f64, f64)) {
        let id = OnlineParameterIdentifier::new(
            furnace_id.clone(),
            defaults.0,
            defaults.1,
            defaults.2,
        );
        self.identifiers.insert(furnace_id, id);
    }

    pub fn process_reading(
        &mut self,
        furnace_id: &str,
        reading: &SensorReading,
        dt: f64,
    ) -> Option<IdentifiedParams> {
        self.identifiers
            .get_mut(furnace_id)
            .map(|id| id.process_reading(reading, dt))
    }

    pub fn get_params(&self, furnace_id: &str) -> Option<IdentifiedParams> {
        self.identifiers.get(furnace_id).map(|id| id.get_params())
    }

    pub fn all_statuses(&self) -> Vec<(String, IdentifiedParams)> {
        self.identifiers
            .iter()
            .map(|(k, v)| (k.clone(), v.get_params()))
            .collect()
    }
}

impl Default for MultiFurnaceIdentifier {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_rls_convergence() {
        let mut rls = RLSState::new(2, 0.995, 10_000.0);

        let true_a = 2.0;
        let true_b = 3.0;

        for i in 0..200 {
            let x = (i as f64) * 0.1;
            let phi = [x, 1.0];
            let y = true_a * x + true_b + (rand::random::<f64>() - 0.5) * 0.1;
            rls.update(&phi, y);
        }

        assert!((rls.theta[0] - true_a).abs() < 0.5, "a={} should ~{}", rls.theta[0], true_a);
        assert!((rls.theta[1] - true_b).abs() < 0.5, "b={} should ~{}", rls.theta[1], true_b);
    }
}
