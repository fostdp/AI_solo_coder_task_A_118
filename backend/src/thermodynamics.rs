use std::collections::VecDeque;

use chrono::{DateTime, Duration, Utc};
use ndarray::{Array1, Array2};
use tracing::{debug, info};

use crate::models::{
    FurnaceConfig, SensorReading, ThermoParams, ThermoPrediction,
};

pub const GAS_CONSTANT_R: f64 = 8.314;
pub const MOLAR_MASS_FE2O3: f64 = 159.69;
pub const MOLAR_MASS_C: f64 = 12.011;
pub const MOLAR_MASS_FE: f64 = 55.845;
pub const MOLAR_MASS_CO: f64 = 28.01;
pub const MOLAR_MASS_CO2: f64 = 44.01;
pub const HEAT_CAPACITY_AIR: f64 = 1005.0;
pub const AIR_DENSITY: f64 = 1.225;
pub const COAL_HEATING_VALUE: f64 = 32_000_000.0;
pub const COMBUSTION_EFFICIENCY: f64 = 0.85;
pub const AMBIENT_TEMP: f64 = 25.0;
pub const SPECIFIC_HEAT_IRON_BED: f64 = 650.0;
pub const IRON_BED_DENSITY: f64 = 3500.0;
pub const NUM_ZONES: usize = 5;

pub struct ThermodynamicsEngine {
    config: FurnaceConfig,
    params: ThermoParams,
    identified_params: crate::parameter_id::IdentifiedParams,
    use_online_id: bool,
    history_len: usize,
    temp_history: VecDeque<f64>,
    temp_zones_history: VecDeque<[f64; NUM_ZONES]>,
    last_update: Option<DateTime<Utc>>,
}

impl ThermodynamicsEngine {
    pub fn new(config: FurnaceConfig, params: ThermoParams) -> Self {
        let identified = crate::parameter_id::IdentifiedParams {
            activation_energy: params.activation_energy,
            pre_exponential_factor: params.pre_exponential_factor,
            heat_loss_coefficient: params.heat_loss_coefficient,
            heat_transfer_coeff: params.heat_conductivity,
            confidence: 0.0,
            sample_count: 0,
            residuals_mse: f64::MAX,
        };
        Self {
            config,
            params,
            identified_params: identified,
            use_online_id: false,
            history_len: 100,
            temp_history: VecDeque::with_capacity(100),
            temp_zones_history: VecDeque::with_capacity(100),
            last_update: None,
        }
    }

    pub fn apply_identified_params(&mut self, identified: &crate::parameter_id::IdentifiedParams) {
        if identified.confidence > 0.35 && identified.sample_count >= 10 {
            self.identified_params = identified.clone();
            self.use_online_id = true;
            self.params.activation_energy = identified.activation_energy;
            self.params.pre_exponential_factor = identified.pre_exponential_factor;
            self.params.heat_loss_coefficient = identified.heat_loss_coefficient;
            self.params.heat_conductivity = identified.heat_transfer_coeff;
        } else {
            self.use_online_id = false;
        }
    }

    pub fn is_using_online_id(&self) -> bool {
        self.use_online_id
    }

    pub fn identified(&self) -> &crate::parameter_id::IdentifiedParams {
        &self.identified_params
    }

    pub fn arrhenius_reaction_rate(
        &self,
        temp: f64,
        o2_conc: f64,
        coal_feed: f64,
    ) -> f64 {
        let (ea, a_factor) = if self.use_online_id {
            (
                self.identified_params.activation_energy,
                self.identified_params.pre_exponential_factor,
            )
        } else {
            (
                self.params.activation_energy,
                self.params.pre_exponential_factor,
            )
        };
        let temp_k = temp + 273.15;
        let exponent = -ea / (GAS_CONSTANT_R * temp_k);
        let base_rate = a_factor * exponent.exp();
        let o2_factor = (o2_conc / 21.0).max(0.1);
        let coal_factor = coal_feed.max(0.01);
        base_rate * o2_factor * coal_factor * 1e-8
    }

    pub fn heat_generated(
        &self,
        reaction_rate: f64,
        coal_feed: f64,
        dt: f64,
    ) -> f64 {
        let heat_reaction = -self.params.reaction_enthalpy * reaction_rate * dt;
        let heat_combustion = coal_feed * dt * COAL_HEATING_VALUE * COMBUSTION_EFFICIENCY;
        heat_reaction + heat_combustion
    }

    pub fn heat_lost(&self, temp: f64, dt: f64) -> f64 {
        let volume = self.config.volume_m3;
        let surface_area = 6.0 * (volume.powf(2.0 / 3.0));
        let delta_t = (temp - AMBIENT_TEMP).max(0.0);
        let hlc = if self.use_online_id {
            self.identified_params.heat_loss_coefficient
        } else {
            self.params.heat_loss_coefficient
        };
        hlc * surface_area * delta_t * dt
    }

    pub fn air_heat_transfer(
        &self,
        air_volume: f64,
        furnace_temp: f64,
        dt: f64,
    ) -> f64 {
        let mass_air = air_volume * dt * AIR_DENSITY;
        let temp_diff = self.params.air_preheat_temp - furnace_temp;
        mass_air * HEAT_CAPACITY_AIR * temp_diff
    }

    pub fn compute_temp_change(
        &self,
        net_heat: f64,
        current_temp: f64,
    ) -> f64 {
        let volume = self.config.volume_m3;
        let total_mass = volume * IRON_BED_DENSITY;
        let specific_heat = self.params.specific_heat;
        if total_mass <= 0.0 || specific_heat <= 0.0 {
            return 0.0;
        }
        net_heat / (total_mass * specific_heat)
    }

    pub fn compute_temp_zones(
        &self,
        avg_temp: f64,
        air_volume: f64,
        reaction_rate: f64,
    ) -> [f64; NUM_ZONES] {
        let base_zones = match self.config.furnace_type {
            crate::models::FurnaceType::HanChaogang => [
                avg_temp - 150.0,
                avg_temp - 90.0,
                avg_temp - 40.0,
                avg_temp + 15.0,
                avg_temp + 50.0,
            ],
            crate::models::FurnaceType::MingBlast => [
                avg_temp - 180.0,
                avg_temp - 110.0,
                avg_temp - 50.0,
                avg_temp + 20.0,
                avg_temp + 65.0,
            ],
        };

        let air_factor = (air_volume * 10.0).clamp(0.0, 50.0);
        let reaction_factor = (reaction_rate * 0.01).clamp(0.0, 30.0);

        [
            (base_zones[0] + air_factor * 0.3).max(AMBIENT_TEMP),
            (base_zones[1] + air_factor * 0.5).max(AMBIENT_TEMP),
            (base_zones[2] + air_factor * 0.7).max(AMBIENT_TEMP),
            (base_zones[3] + reaction_factor * 0.8).max(AMBIENT_TEMP),
            (base_zones[4] + reaction_factor).max(AMBIENT_TEMP),
        ]
    }

    pub fn compute_co_concentration(
        &self,
        coal_feed: f64,
        iron_feed: f64,
        o2_conc: f64,
        reaction_rate: f64,
    ) -> f64 {
        let stoich_ratio = 0.55;
        let actual_ratio = if iron_feed > 0.001 {
            coal_feed / iron_feed
        } else {
            0.0
        };
        let excess_carbon = (actual_ratio - stoich_ratio).max(0.0);
        let co_from_excess = excess_carbon * 6.0;
        let co_from_reaction = reaction_rate * 0.0008;
        let o2_inhibition = (1.0 - (o2_conc / 21.0).clamp(0.0, 0.9)).powf(1.5);

        (co_from_excess + co_from_reaction) * (0.3 + 0.7 * o2_inhibition)
    }

    pub fn compute_iron_output_rate(
        &self,
        reaction_rate: f64,
        temp: f64,
        target_min: f64,
    ) -> f64 {
        let temp_factor = if temp < target_min * 0.8 {
            0.1
        } else if temp < target_min {
            0.5
        } else {
            1.0
        };
        let fe_mol_per_reaction = 2.0;
        reaction_rate * fe_mol_per_reaction * MOLAR_MASS_FE * temp_factor / 1000.0
    }

    pub fn compute_energy_efficiency(
        &self,
        net_heat: f64,
        coal_feed: f64,
        dt: f64,
    ) -> f64 {
        let total_input = coal_feed * dt * COAL_HEATING_VALUE;
        if total_input <= 0.0 {
            return 0.0;
        }
        (net_heat / total_input * 100.0).clamp(0.0, 95.0)
    }

    pub fn update_with_reading(&mut self, reading: &SensorReading) {
        self.temp_history.push_back(reading.furnace_temp);
        if self.temp_history.len() > self.history_len {
            self.temp_history.pop_front();
        }

        self.temp_zones_history.push_back(reading.temp_zones());
        if self.temp_zones_history.len() > self.history_len {
            self.temp_zones_history.pop_front();
        }

        self.last_update = Some(reading.timestamp);
    }

    pub fn predict_next(
        &mut self,
        current: &SensorReading,
        proposed_frequency: f64,
        proposed_stroke: f64,
        dt: f64,
    ) -> ThermoPrediction {
        let bore_area = match self.config.furnace_type {
            crate::models::FurnaceType::HanChaogang => 0.08,
            crate::models::FurnaceType::MingBlast => 0.15,
        };

        let new_air_volume =
            Self::calc_air_volume(proposed_frequency, proposed_stroke, bore_area);
        let new_pressure =
            Self::calc_wind_pressure(proposed_frequency, proposed_stroke);

        let reaction_rate = self.arrhenius_reaction_rate(
            current.furnace_temp,
            current.o2_concentration,
            current.coal_feed_rate,
        );

        let heat_in = self.heat_generated(reaction_rate, current.coal_feed_rate, dt);
        let air_heat =
            self.air_heat_transfer(new_air_volume, current.furnace_temp, dt);
        let heat_out = self.heat_lost(current.furnace_temp, dt);

        let net_heat = heat_in + air_heat - heat_out;
        let delta_temp = self.compute_temp_change(net_heat, current.furnace_temp);
        let predicted_temp = (current.furnace_temp + delta_temp)
            .clamp(AMBIENT_TEMP, self.config.max_temperature + 200.0);

        let predicted_zones = self.compute_temp_zones(
            predicted_temp,
            new_air_volume,
            reaction_rate,
        );

        let predicted_co = self.compute_co_concentration(
            current.coal_feed_rate,
            current.iron_feed_rate,
            current.o2_concentration * 0.95,
            reaction_rate,
        );

        let iron_rate = self.compute_iron_output_rate(
            reaction_rate,
            predicted_temp,
            self.config.target_temp_min,
        );

        let efficiency = self.compute_energy_efficiency(net_heat, current.coal_feed_rate, dt);

        let conf = self.calculate_confidence();

        ThermoPrediction {
            timestamp: Utc::now() + Duration::seconds(dt as i64),
            furnace_id: current.furnace_id.clone(),
            predicted_temp,
            predicted_co,
            predicted_reaction_rate: reaction_rate,
            predicted_efficiency: efficiency,
            temp_distribution: predicted_zones.to_vec(),
            iron_output_rate: iron_rate,
            confidence: conf,
        }
    }

    pub fn simulate_temp_field(
        &self,
        zones: [f64; NUM_ZONES],
        resolution: (usize, usize),
    ) -> Array2<f64> {
        let (rows, cols) = resolution;
        let mut field = Array2::zeros((rows, cols));
        let zone_heights = [0.0, 0.2, 0.4, 0.65, 0.85, 1.0];

        for r in 0..rows {
            let ry = 1.0 - (r as f64 / (rows - 1).max(1) as f64);
            let zone_idx = (0..NUM_ZONES)
                .find(|&i| ry >= zone_heights[i] && ry < zone_heights[i + 1])
                .unwrap_or(NUM_ZONES - 1);
            let zone_temp = zones[zone_idx];

            for c in 0..cols {
                let cx = (c as f64 / (cols - 1).max(1) as f64) - 0.5;
                let radial = (cx * 2.0).abs();
                let edge_factor = 1.0 - radial * 0.3;
                let noise = (ry * 13.7 + cx * 7.3).sin() * 8.0;

                field[[r, c]] = zone_temp * edge_factor + noise;
            }
        }

        for _ in 0..3 {
            field = Self::gaussian_smooth(&field);
        }

        field
    }

    fn gaussian_smooth(input: &Array2<f64>) -> Array2<f64> {
        let (rows, cols) = input.dim();
        let mut output = Array2::zeros((rows, cols));
        let kernel = [
            [1.0, 2.0, 1.0],
            [2.0, 4.0, 2.0],
            [1.0, 2.0, 1.0],
        ];
        let kernel_sum: f64 = kernel.iter().flatten().sum();

        for r in 0..rows {
            for c in 0..cols {
                let mut val = 0.0;
                for kr in 0..3 {
                    for kc in 0..3 {
                        let rr = (r as isize + kr as isize - 1).clamp(0, rows as isize - 1) as usize;
                        let cc = (c as isize + kc as isize - 1).clamp(0, cols as isize - 1) as usize;
                        val += input[[rr, cc]] * kernel[kr][kc];
                    }
                }
                output[[r, c]] = val / kernel_sum;
            }
        }

        output
    }

    pub fn calc_air_volume(frequency: f64, stroke: f64, bore_area: f64) -> f64 {
        let cycles_per_sec = frequency / 60.0;
        let stroke_m = stroke / 100.0;
        let volume_per_cycle = bore_area * stroke_m * 2.0;
        cycles_per_sec * volume_per_cycle * 0.75
    }

    pub fn calc_wind_pressure(frequency: f64, stroke: f64) -> f64 {
        let velocity = (stroke / 100.0) * (frequency / 60.0) * 2.0;
        3.5 * velocity * velocity * 0.5 * AIR_DENSITY * 1000.0
    }

    pub fn mass_balance_check(
        &self,
        iron_in: f64,
        coal_in: f64,
        air_in: f64,
        iron_out: f64,
        gas_out: f64,
        slag_out: f64,
    ) -> f64 {
        let total_in = iron_in + coal_in + air_in * AIR_DENSITY;
        let total_out = iron_out + gas_out + slag_out;
        if total_in <= 0.0 {
            return 0.0;
        }
        (total_in - total_out).abs() / total_in
    }

    pub fn energy_balance_check(
        &self,
        chem_energy_in: f64,
        preheat_energy_in: f64,
        useful_energy_out: f64,
        heat_loss: f64,
        exhaust_heat: f64,
    ) -> f64 {
        let total_in = chem_energy_in + preheat_energy_in;
        let total_out = useful_energy_out + heat_loss + exhaust_heat;
        if total_in <= 0.0 {
            return 0.0;
        }
        (total_in - total_out).abs() / total_in
    }

    fn calculate_confidence(&self) -> f64 {
        let base = if self.temp_history.len() >= 20 {
            0.9
        } else if self.temp_history.len() >= 10 {
            0.75
        } else if self.temp_history.len() >= 5 {
            0.5
        } else {
            0.3
        };

        if self.temp_history.len() >= 2 {
            let recent: Vec<&f64> = self.temp_history.iter().rev().take(10).collect();
            if recent.len() >= 2 {
                let mean: f64 = recent.iter().map(|&&v| v).sum::<f64>() / recent.len() as f64;
                let var: f64 = recent
                    .iter()
                    .map(|&&v| (v - mean).powi(2))
                    .sum::<f64>()
                    / recent.len() as f64;
                let std_dev = var.sqrt();
                let stability = (1.0 - (std_dev / mean.max(1.0) * 10.0)).clamp(0.0, 1.0);
                return (base * 0.7 + stability * 0.3).clamp(0.1, 0.99);
            }
        }
        base
    }

    pub fn temp_trend(&self) -> f64 {
        if self.temp_history.len() < 5 {
            return 0.0;
        }
        let n = self.temp_history.len().min(20);
        let recent: Vec<f64> = self.temp_history.iter().rev().take(n).copied().collect();
        let xs: Array1<f64> = Array1::range(0.0, n as f64, 1.0);
        let ys = Array1::from_vec(recent);
        let x_mean = xs.mean().unwrap_or(0.0);
        let y_mean = ys.mean().unwrap_or(0.0);
        let num: f64 = xs.iter().zip(ys.iter()).map(|(x, y)| (x - x_mean) * (y - y_mean)).sum();
        let den: f64 = xs.iter().map(|x| (x - x_mean).powi(2)).sum();
        if den.abs() < 1e-10 { 0.0 } else { num / den }
    }

    pub fn get_config(&self) -> &FurnaceConfig {
        &self.config
    }

    pub fn get_params(&self) -> &ThermoParams {
        &self.params
    }

    pub fn update_params(&mut self, params: ThermoParams) {
        self.params = params;
    }
}

pub struct MultiFurnaceThermoEngine {
    engines: std::collections::HashMap<String, ThermodynamicsEngine>,
}

impl MultiFurnaceThermoEngine {
    pub fn new() -> Self {
        Self {
            engines: std::collections::HashMap::new(),
        }
    }

    pub fn add_furnace(&mut self, config: FurnaceConfig, params: ThermoParams) {
        let engine = ThermodynamicsEngine::new(config, params);
        self.engines.insert(engine.get_config().furnace_id.clone(), engine);
    }

    pub fn get_engine(&self, furnace_id: &str) -> Option<&ThermodynamicsEngine> {
        self.engines.get(furnace_id)
    }

    pub fn get_engine_mut(&mut self, furnace_id: &str) -> Option<&mut ThermodynamicsEngine> {
        self.engines.get_mut(furnace_id)
    }

    pub fn furnace_ids(&self) -> Vec<String> {
        self.engines.keys().cloned().collect()
    }

    pub fn predict_all(
        &mut self,
        readings: &[SensorReading],
        dt: f64,
    ) -> Vec<ThermoPrediction> {
        let mut preds = Vec::new();
        for r in readings {
            if let Some(engine) = self.engines.get_mut(&r.furnace_id) {
                let pred = engine.predict_next(
                    r,
                    r.push_pull_frequency,
                    r.stroke_length,
                    dt,
                );
                preds.push(pred);
                engine.update_with_reading(r);
            }
        }
        preds
    }
}

impl Default for MultiFurnaceThermoEngine {
    fn default() -> Self {
        Self::new()
    }
}

pub fn temp_to_rgb(temp: f64, temp_min: f64, temp_max: f64) -> (u8, u8, u8) {
    let t = ((temp - temp_min) / (temp_max - temp_min)).clamp(0.0, 1.0);

    let r = if t < 0.25 {
        80.0 + t * 4.0 * 100.0
    } else if t < 0.5 {
        180.0 + (t - 0.25) * 4.0 * 75.0
    } else {
        255.0
    };

    let g = if t < 0.33 {
        t * 3.0 * 180.0
    } else if t < 0.66 {
        180.0 + (t - 0.33) * 3.0 * 60.0
    } else {
        240.0 - (t - 0.66) * 3.0 * 200.0
    };

    let b = if t < 0.5 {
        200.0 - t * 2.0 * 200.0
    } else {
        0.0
    };

    (r.clamp(0.0, 255.0) as u8, g.clamp(0.0, 255.0) as u8, b.clamp(0.0, 255.0) as u8)
}

pub fn temp_to_hex(temp: f64, temp_min: f64, temp_max: f64) -> String {
    let (r, g, b) = temp_to_rgb(temp, temp_min, temp_max);
    format!("#{:02X}{:02X}{:02X}", r, g, b)
}
