use metrics::{counter, describe_counter, describe_gauge, gauge, Unit};
use once_cell::sync::Lazy;

pub static METRICS_INITIALIZED: Lazy<std::sync::atomic::AtomicBool> =
    Lazy::new(|| std::sync::atomic::AtomicBool::new(false));

pub fn init_metrics() {
    if METRICS_INITIALIZED.swap(true, std::sync::atomic::Ordering::SeqCst) {
        return;
    }

    describe_counter!(
        "metallurgy_sensor_readings_total",
        Unit::Count,
        "Total number of sensor readings received"
    );
    describe_counter!(
        "metallurgy_sensor_readings_valid_total",
        Unit::Count,
        "Total number of validated sensor readings"
    );
    describe_counter!(
        "metallurgy_sensor_readings_invalid_total",
        Unit::Count,
        "Total number of invalid sensor readings"
    );
    describe_counter!(
        "metallurgy_thermo_predictions_total",
        Unit::Count,
        "Total number of thermodynamic predictions"
    );
    describe_counter!(
        "metallurgy_control_actions_total",
        Unit::Count,
        "Total number of control actions issued"
    );
    describe_counter!(
        "metallurgy_alarm_events_total",
        Unit::Count,
        "Total number of alarm events"
    );
    describe_counter!(
        "metallurgy_mqtt_publishes_total",
        Unit::Count,
        "Total number of MQTT publishes"
    );
    describe_counter!(
        "metallurgy_mqtt_publish_errors_total",
        Unit::Count,
        "Total number of MQTT publish errors"
    );
    describe_counter!(
        "metallurgy_http_requests_total",
        Unit::Count,
        "Total number of HTTP requests"
    );

    describe_gauge!(
        "metallurgy_furnace_temperature_celsius",
        Unit::DegreesCelsius,
        "Current furnace temperature"
    );
    describe_gauge!(
        "metallurgy_furnace_co_concentration_ratio",
        Unit::Percent,
        "Current CO concentration in percent"
    );
    describe_gauge!(
        "metallurgy_bellows_frequency_per_minute",
        Unit::Hertz,
        "Current bellows push-pull frequency"
    );
    describe_gauge!(
        "metallurgy_bellows_stroke_cm",
        Unit::Centimeters,
        "Current bellows stroke length"
    );
    describe_gauge!(
        "metallurgy_energy_efficiency_ratio",
        Unit::Percent,
        "Current energy efficiency"
    );
    describe_gauge!(
        "metallurgy_active_alarms",
        Unit::Count,
        "Current number of active unacknowledged alarms"
    );

    tracing::info!("[metrics] Prometheus metrics descriptors registered");
}

#[inline]
pub fn inc_sensor_readings(furnace_id: &str) {
    counter!("metallurgy_sensor_readings_total", "furnace_id" => furnace_id.to_string()).increment(1);
}

#[inline]
pub fn inc_sensor_valid(furnace_id: &str) {
    counter!("metallurgy_sensor_readings_valid_total", "furnace_id" => furnace_id.to_string()).increment(1);
}

#[inline]
pub fn inc_sensor_invalid(furnace_id: &str, reason: &str) {
    counter!("metallurgy_sensor_readings_invalid_total",
        "furnace_id" => furnace_id.to_string(),
        "reason" => reason.to_string()
    ).increment(1);
}

#[inline]
pub fn inc_thermo_predictions(furnace_id: &str, algo: &str) {
    counter!("metallurgy_thermo_predictions_total",
        "furnace_id" => furnace_id.to_string(),
        "algo" => algo.to_string()
    ).increment(1);
}

#[inline]
pub fn inc_control_actions(furnace_id: &str, mode: &str) {
    counter!("metallurgy_control_actions_total",
        "furnace_id" => furnace_id.to_string(),
        "mode" => mode.to_string()
    ).increment(1);
}

#[inline]
pub fn inc_alarm(furnace_id: &str, alarm_type: &str, level: &str) {
    counter!("metallurgy_alarm_events_total",
        "furnace_id" => furnace_id.to_string(),
        "alarm_type" => alarm_type.to_string(),
        "level" => level.to_string()
    ).increment(1);
}

#[inline]
pub fn inc_mqtt_publish_ok() {
    counter!("metallurgy_mqtt_publishes_total").increment(1);
}

#[inline]
pub fn inc_mqtt_publish_err() {
    counter!("metallurgy_mqtt_publish_errors_total").increment(1);
}

#[inline]
pub fn set_furnace_temp(furnace_id: &str, temp: f64) {
    gauge!("metallurgy_furnace_temperature_celsius", "furnace_id" => furnace_id.to_string()).set(temp);
}

#[inline]
pub fn set_co_conc(furnace_id: &str, co: f64) {
    gauge!("metallurgy_furnace_co_concentration_ratio", "furnace_id" => furnace_id.to_string()).set(co);
}

#[inline]
pub fn set_frequency(furnace_id: &str, freq: f64) {
    gauge!("metallurgy_bellows_frequency_per_minute", "furnace_id" => furnace_id.to_string()).set(freq);
}

#[inline]
pub fn set_stroke(furnace_id: &str, stroke: f64) {
    gauge!("metallurgy_bellows_stroke_cm", "furnace_id" => furnace_id.to_string()).set(stroke);
}

#[inline]
pub fn set_energy_eff(furnace_id: &str, eff: f64) {
    gauge!("metallurgy_energy_efficiency_ratio", "furnace_id" => furnace_id.to_string()).set(eff);
}

#[inline]
pub fn set_active_alarms(count: f64) {
    gauge!("metallurgy_active_alarms").set(count);
}
