use std::collections::HashMap;
use std::sync::Arc;

use axum::{
    extract::{
        ws::{Message, WebSocket, WebSocketUpgrade},
        ConnectInfo, Extension, Path, Query, State,
    },
    http::StatusCode,
    response::IntoResponse,
    Json, Router,
    routing::{get, post, put, delete},
};
use chrono::{Duration, Utc};
use dashmap::DashMap;
use futures_util::{SinkExt, StreamExt};
use serde::{Deserialize, Serialize};
use tokio::sync::broadcast;
use tracing::{debug, error, info, warn};

use crate::models::*;
use crate::storage::ClickHouseStore;
use crate::thermodynamics::{MultiFurnaceThermoEngine, temp_to_hex};
use crate::rl_control::MultiFurnaceRLController;
use crate::mqtt::{AlarmDetector, MqttPublisher, MqttAlarmMessage};

type SharedState = Arc<AppState>;

pub struct AppState {
    pub store: ClickHouseStore,
    pub thermo_engine: tokio::sync::RwLock<MultiFurnaceThermoEngine>,
    pub rl_controller: MultiFurnaceRLController,
    pub alarm_detector: tokio::sync::Mutex<AlarmDetector>,
    pub mqtt_publisher: MqttPublisher,
    pub ws_sessions: DashMap<String, broadcast::Sender<WSMessage>>,
    pub sensor_broadcast: broadcast::Sender<SensorReading>,
    pub alarm_broadcast: broadcast::Sender<AlarmEvent>,
    pub last_readings: DashMap<String, SensorReading>,
    pub prev_temps: DashMap<String, f64>,
}

impl AppState {
    pub fn new(
        store: ClickHouseStore,
        thermo_engine: MultiFurnaceThermoEngine,
        rl_controller: MultiFurnaceRLController,
        alarm_detector: AlarmDetector,
        mqtt_publisher: MqttPublisher,
    ) -> Self {
        let (sensor_tx, _) = broadcast::channel(2000);
        let (alarm_tx, _) = broadcast::channel(1000);

        Self {
            store,
            thermo_engine: tokio::sync::RwLock::new(thermo_engine),
            rl_controller,
            alarm_detector: tokio::sync::Mutex::new(alarm_detector),
            mqtt_publisher,
            ws_sessions: DashMap::new(),
            sensor_broadcast: sensor_tx,
            alarm_broadcast: alarm_tx,
            last_readings: DashMap::new(),
            prev_temps: DashMap::new(),
        }
    }
}

#[derive(Debug, Deserialize)]
pub struct RangeQuery {
    pub start: Option<String>,
    pub end: Option<String>,
    pub limit: Option<u64>,
    pub hours: Option<u32>,
}

#[derive(Debug, Deserialize)]
pub struct PaginationQuery {
    pub limit: Option<u64>,
    pub offset: Option<u64>,
}

#[derive(Debug, Deserialize)]
pub struct TempFieldQuery {
    pub resolution: Option<usize>,
}

#[derive(Debug, Serialize)]
pub struct TempFieldResponse {
    pub furnace_id: String,
    pub resolution: (usize, usize),
    pub temp_min: f64,
    pub temp_max: f64,
    pub zones: [f64; 5],
    pub field_data: Vec<Vec<f64>>,
    pub color_data: Vec<Vec<String>>,
    pub timestamp: chrono::DateTime<Utc>,
}

#[derive(Debug, Serialize)]
pub struct SystemStatus {
    pub uptime_seconds: u64,
    pub furnaces: Vec<FurnaceConfig>,
    pub active_connections: usize,
    pub total_sensor_records: u64,
    pub clickhouse_connected: bool,
    pub mqtt_connected: bool,
    pub rl_status: Vec<crate::rl_control::RLStatus>,
}

pub fn build_router(state: SharedState) -> Router {
    Router::new()
        .route("/", get(root_handler))
        .route("/api/health", get(health_check))
        .route("/api/status", get(get_system_status))
        .nest("/api/furnaces", furnaces_routes())
        .nest("/api/sensor", sensor_routes())
        .nest("/api/thermo", thermo_routes())
        .nest("/api/alarms", alarms_routes())
        .nest("/api/rl", rl_routes())
        .route("/ws", get(ws_handler))
        .layer(Extension(state))
}

fn furnaces_routes() -> Router<SharedState> {
    Router::new()
        .route("/", get(list_furnaces))
        .route("/:furnace_id", get(get_furnace))
        .route("/:furnace_id/reading/latest", get(get_latest_reading))
        .route("/:furnace_id/reading/history", get(get_reading_history))
        .route("/:furnace_id/temp_field", get(get_temp_field))
        .route("/:furnace_id/production", get(get_production_stats))
}

fn sensor_routes() -> Router<SharedState> {
    Router::new()
        .route("/report", post(report_sensor_data))
        .route("/batch", post(batch_report))
}

fn thermo_routes() -> Router<SharedState> {
    Router::new()
        .route("/predict/:furnace_id", post(get_thermo_prediction))
        .route("/params/:furnace_id", get(get_thermo_params).put(set_thermo_params))
}

fn alarms_routes() -> Router<SharedState> {
    Router::new()
        .route("/", get(list_alarms))
        .route("/:event_id/ack", put(acknowledge_alarm))
}

fn rl_routes() -> Router<SharedState> {
    Router::new()
        .route("/status", get(get_rl_status))
        .route("/status/:furnace_id", get(get_rl_status_for_furnace))
        .route("/action/:furnace_id", get(get_current_action).post(set_manual_action))
}

async fn root_handler() -> impl IntoResponse {
    Json(serde_json::json!({
        "service": "古代风箱鼓风冶铁过程热力学模拟与炉温控制仿真系统",
        "version": "0.1.0",
        "endpoints": {
            "health": "/api/health",
            "furnaces": "/api/furnaces/",
            "sensor_report": "POST /api/sensor/report",
            "alarms": "/api/alarms/",
            "rl_status": "/api/rl/status",
            "ws": "/ws"
        }
    }))
}

async fn health_check(State(state): State<SharedState>) -> impl IntoResponse {
    let ch_ok = state.store.ping().await.unwrap_or(false);
    let status = if ch_ok { "healthy" } else { "degraded" };
    let code = if ch_ok { StatusCode::OK } else { StatusCode::SERVICE_UNAVAILABLE };

    (code, Json(serde_json::json!({
        "status": status,
        "timestamp": Utc::now().to_rfc3339(),
        "components": {
            "clickhouse": ch_ok,
            "ws_sessions": state.ws_sessions.len(),
            "sensor_broadcast_receivers": state.sensor_broadcast.receiver_count(),
        }
    })))
}

async fn get_system_status(State(state): State<SharedState>) -> impl IntoResponse {
    let furnaces = state.store.get_furnace_configs().await.unwrap_or_default();
    let ch_ok = state.store.ping().await.unwrap_or(false);

    let response = SystemStatus {
        uptime_seconds: 0,
        furnaces,
        active_connections: state.ws_sessions.len(),
        total_sensor_records: state.last_readings.len() as u64,
        clickhouse_connected: ch_ok,
        mqtt_connected: true,
        rl_status: state.rl_controller.get_all_status(),
    };

    Json(ApiResponse::ok(response))
}

async fn list_furnaces(State(state): State<SharedState>) -> impl IntoResponse {
    match state.store.get_furnace_configs().await {
        Ok(configs) => Json(ApiResponse::ok(configs)),
        Err(e) => {
            error!("查询炉列表失败: {}", e);
            Json(ApiResponse::error(&format!("查询失败: {}", e)))
        }
    }
}

async fn get_furnace(
    State(state): State<SharedState>,
    Path(furnace_id): Path<String>,
) -> impl IntoResponse {
    match state.store.get_furnace_config(&furnace_id).await {
        Ok(Some(config)) => Json(ApiResponse::ok(config)),
        Ok(None) => Json(ApiResponse::error("未找到该冶炼炉")),
        Err(e) => Json(ApiResponse::error(&format!("查询失败: {}", e))),
    }
}

async fn get_latest_reading(
    State(state): State<SharedState>,
    Path(furnace_id): Path<String>,
) -> impl IntoResponse {
    if let Some(cached) = state.last_readings.get(&furnace_id) {
        return Json(ApiResponse::ok(cached.clone()));
    }

    match state.store.get_latest_reading(&furnace_id).await {
        Ok(Some(reading)) => Json(ApiResponse::ok(reading)),
        Ok(None) => Json(ApiResponse::error("暂无数据")),
        Err(e) => Json(ApiResponse::error(&format!("查询失败: {}", e))),
    }
}

async fn get_reading_history(
    State(state): State<SharedState>,
    Path(furnace_id): Path<String>,
    Query(query): Query<RangeQuery>,
) -> impl IntoResponse {
    let limit = query.limit.unwrap_or(500).min(10000);
    let (start, end) = if let Some(hours) = query.hours {
        (Utc::now() - Duration::hours(hours as i64), Utc::now())
    } else {
        let s = query.start
            .and_then(|s| chrono::DateTime::parse_from_rfc3339(&s).ok())
            .map(|d| d.with_timezone(&Utc))
            .unwrap_or_else(|| Utc::now() - Duration::hours(1));
        let e = query.end
            .and_then(|s| chrono::DateTime::parse_from_rfc3339(&s).ok())
            .map(|d| d.with_timezone(&Utc))
            .unwrap_or_else(|| Utc::now());
        (s, e)
    };

    match state.store.get_readings_range(&furnace_id, start, end, limit).await {
        Ok(readings) => Json(ApiResponse::ok(readings)),
        Err(e) => Json(ApiResponse::error(&format!("查询失败: {}", e))),
    }
}

async fn get_temp_field(
    State(state): State<SharedState>,
    Path(furnace_id): Path<String>,
    Query(query): Query<TempFieldQuery>,
) -> impl IntoResponse {
    let res = query.resolution.unwrap_or(64).clamp(16, 256);
    let resolution = (res, res);

    let reading = state.last_readings.get(&furnace_id)
        .map(|r| r.clone())
        .or_else(|| match futures::executor::block_on(state.store.get_latest_reading(&furnace_id)) {
            Ok(r) => r,
            Err(_) => None,
        });

    let reading = match reading {
        Some(r) => r,
        None => return Json(ApiResponse::error("暂无传感器数据")),
    };

    let zones = reading.temp_zones();
    let temp_min = zones.iter().cloned().fold(f64::INFINITY, f64::min) - 20.0;
    let temp_max = zones.iter().cloned().fold(f64::NEG_INFINITY, f64::max) + 20.0;

    let engine = state.thermo_engine.read().await;
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
    let mut color_data = Vec::with_capacity(resolution.0);
    for r in 0..resolution.0 {
        let mut row_data = Vec::with_capacity(resolution.1);
        let mut row_colors = Vec::with_capacity(resolution.1);
        for c in 0..resolution.1 {
            let t = field[[r, c]];
            row_data.push(t);
            row_colors.push(temp_to_hex(t, temp_min, temp_max));
        }
        field_data.push(row_data);
        color_data.push(row_colors);
    }

    let response = TempFieldResponse {
        furnace_id: furnace_id.clone(),
        resolution,
        temp_min,
        temp_max,
        zones,
        field_data,
        color_data,
        timestamp: reading.timestamp,
    };

    Json(ApiResponse::ok(response))
}

async fn get_production_stats(
    State(state): State<SharedState>,
    Path(furnace_id): Path<String>,
    Query(query): Query<RangeQuery>,
) -> impl IntoResponse {
    let days = query.hours.map(|h| (h / 24).max(1)).unwrap_or(30);
    match state.store.get_production_stats(&furnace_id, days).await {
        Ok(stats) => Json(ApiResponse::ok(stats)),
        Err(e) => Json(ApiResponse::error(&format!("查询失败: {}", e))),
    }
}

async fn report_sensor_data(
    State(state): State<SharedState>,
    Json(reading): Json<SensorReading>,
) -> impl IntoResponse {
    debug!("收到传感器数据: furnace={}, temp={:.1}", reading.furnace_id, reading.furnace_temp);

    let furnace_id = reading.furnace_id.clone();

    let config = match state.store.get_furnace_config(&furnace_id).await {
        Ok(Some(c)) => c,
        Ok(None) => {
            warn!("未找到炉配置: {}, 使用默认", furnace_id);
            FurnaceConfig {
                furnace_id: furnace_id.clone(),
                furnace_name: furnace_id.clone(),
                furnace_type: FurnaceType::HanChaogang,
                volume_m3: 2.5,
                max_temperature: 1450.0,
                target_temp_min: 1200.0,
                target_temp_max: 1350.0,
            }
        }
        Err(e) => {
            return Json(ApiResponse::<serde_json::Value>::error(&format!("配置查询失败: {}", e)));
        }
    };

    if let Err(e) = state.store.insert_sensor_reading(&reading).await {
        warn!("存储传感器数据失败: {}", e);
    }

    state.last_readings.insert(furnace_id.clone(), reading.clone());

    let prev_temp = state.prev_temps.get(&furnace_id).map(|r| *r).unwrap_or(reading.furnace_temp);
    state.prev_temps.insert(furnace_id.clone(), reading.furnace_temp);

    let (rl_action, control_step) = state.rl_controller.process_reading(&reading, &config, prev_temp);

    if let Some(step) = control_step {
        if let Err(e) = state.store.insert_control_step(&step).await {
            warn!("存储RL控制步骤失败: {}", e);
        }
    }

    let mut alarms_generated = Vec::new();
    {
        let mut detector = state.alarm_detector.lock().await;
        let alarms = detector.detect_from_reading(&reading);
        for alarm in &alarms {
            info!("检测到告警: furnace={}, type={:?}, level={:?}",
                alarm.furnace_id, alarm.alarm_type, alarm.alarm_level);

            if let Err(e) = state.store.insert_alarm(alarm).await {
                warn!("存储告警失败: {}", e);
            }

            if let Err(e) = state.mqtt_publisher.publish_alarm(alarm).await {
                warn!("MQTT发布告警失败: {}", e);
            }

            let _ = state.alarm_broadcast.send(alarm.clone());
            alarms_generated.push(alarm.clone());
        }
    }

    {
        let mut engine = state.thermo_engine.write().await;
        if let Some(e) = engine.get_engine_mut(&furnace_id) {
            e.update_with_reading(&reading);
        }
    }

    let _ = state.sensor_broadcast.send(reading.clone());

    let broadcast_msg = WSMessage::sensor(&reading);
    for session in state.ws_sessions.iter() {
        let _ = session.value().send(broadcast_msg.clone());
    }

    for alarm in &alarms_generated {
        let alarm_msg = WSMessage::alarm(alarm);
        for session in state.ws_sessions.iter() {
            let _ = session.value().send(alarm_msg.clone());
        }
    }

    let action_msg = WSMessage::action(&furnace_id, &rl_action);
    for session in state.ws_sessions.iter() {
        let _ = session.value().send(action_msg.clone());
    }

    let mut resp = ApiResponse::ok_with_action(serde_json::json!({
        "stored": true,
        "timestamp": reading.timestamp.to_rfc3339(),
        "furnace": furnace_id,
    }), rl_action);

    if !alarms_generated.is_empty() {
        resp.alarms = Some(alarms_generated);
    }

    Json(resp)
}

async fn batch_report(
    State(state): State<SharedState>,
    Json(readings): Json<Vec<SensorReading>>,
) -> impl IntoResponse {
    let mut results = Vec::new();
    let mut errors = Vec::new();

    for reading in readings {
        let result = futures::executor::block_on(report_sensor_data(
            State(state.clone()),
            Json(reading),
        ));
        results.push(result);
    }

    Json(ApiResponse::ok(serde_json::json!({
        "total": readings.len(),
        "errors": errors.len(),
    })))
}

async fn get_thermo_prediction(
    State(state): State<SharedState>,
    Path(furnace_id): Path<String>,
    Json(action): Json<RLAction>,
) -> impl IntoResponse {
    let reading = match state.last_readings.get(&furnace_id) {
        Some(r) => r.clone(),
        None => {
            return Json(ApiResponse::error("暂无传感器数据，无法预测"));
        }
    };

    let mut engine = state.thermo_engine.write().await;
    let prediction = match engine.get_engine_mut(&furnace_id) {
        Some(e) => e.predict_next(&reading, action.frequency, action.stroke, 10.0),
        None => return Json(ApiResponse::error("未找到热力学引擎")),
    };

    Json(ApiResponse::ok(prediction))
}

async fn get_thermo_params(
    State(state): State<SharedState>,
    Path(furnace_id): Path<String>,
) -> impl IntoResponse {
    let params = state.thermo_engine.read().await
        .get_engine(&furnace_id)
        .map(|e| e.get_params().clone());

    if let Some(p) = params {
        Json(ApiResponse::ok(p))
    } else {
        match state.store.get_thermo_params(&furnace_id).await {
            Ok(Some(p)) => Json(ApiResponse::ok(p)),
            Ok(None) => Json(ApiResponse::error("未找到热力学参数")),
            Err(e) => Json(ApiResponse::error(&format!("查询失败: {}", e))),
        }
    }
}

async fn set_thermo_params(
    State(state): State<SharedState>,
    Path(furnace_id): Path<String>,
    Json(params): Json<ThermoParams>,
) -> impl IntoResponse {
    let params_with_id = ThermoParams {
        furnace_id: furnace_id.clone(),
        ..params
    };

    if let Err(e) = state.store.insert_thermo_params(&params_with_id).await {
        warn!("存储热力学参数失败: {}", e);
    }

    let mut engine = state.thermo_engine.write().await;
    if let Some(e) = engine.get_engine_mut(&furnace_id) {
        e.update_params(params_with_id.clone());
    }

    Json(ApiResponse::ok(params_with_id))
}

async fn list_alarms(
    State(state): State<SharedState>,
    Query(query): Query<RangeQuery>,
) -> impl IntoResponse {
    let hours = query.hours.unwrap_or(24);
    let furnace_id: Option<&str> = None;

    match state.store.get_active_alarms(furnace_id, hours).await {
        Ok(alarms) => Json(ApiResponse::ok(alarms)),
        Err(e) => Json(ApiResponse::error(&format!("查询失败: {}", e))),
    }
}

async fn acknowledge_alarm(
    State(state): State<SharedState>,
    Path(event_id): Path<String>,
) -> impl IntoResponse {
    match state.store.acknowledge_alarm(&event_id).await {
        Ok(n) if n > 0 => Json(ApiResponse::ok(serde_json::json!({"acknowledged": true}))),
        Ok(_) => Json(ApiResponse::error("未找到该告警或已确认")),
        Err(e) => Json(ApiResponse::error(&format!("操作失败: {}", e))),
    }
}

async fn get_rl_status(State(state): State<SharedState>) -> impl IntoResponse {
    Json(ApiResponse::ok(state.rl_controller.get_all_status()))
}

async fn get_rl_status_for_furnace(
    State(state): State<SharedState>,
    Path(furnace_id): Path<String>,
) -> impl IntoResponse {
    match state.rl_controller.get_trainer_status(&furnace_id) {
        Some(s) => Json(ApiResponse::ok(s)),
        None => Json(ApiResponse::error("未找到该炉的RL控制器")),
    }
}

async fn get_current_action(
    State(state): State<SharedState>,
    Path(furnace_id): Path<String>,
) -> impl IntoResponse {
    match state.last_readings.get(&furnace_id) {
        Some(r) => {
            let action = RLAction {
                frequency: r.push_pull_frequency,
                stroke: r.stroke_length,
            };
            Json(ApiResponse::ok(action))
        }
        None => Json(ApiResponse::error("暂无当前动作数据")),
    }
}

async fn set_manual_action(
    State(state): State<SharedState>,
    Path(_furnace_id): Path<String>,
    Json(_action): Json<RLAction>,
) -> impl IntoResponse {
    Json(ApiResponse::ok(serde_json::json!({"manual_override": true})))
}

async fn ws_handler(
    ws: WebSocketUpgrade,
    ConnectInfo(addr): ConnectInfo<std::net::SocketAddr>,
    Extension(state): Extension<SharedState>,
) -> impl IntoResponse {
    info!("新的WebSocket连接: {}", addr);
    ws.on_upgrade(move |socket| handle_websocket(socket, addr, state))
}

async fn handle_websocket(socket: WebSocket, addr: std::net::SocketAddr, state: SharedState) {
    let (mut sender, mut receiver) = socket.split();
    let session_id = format!("ws-{}-{}", addr, rand::random::<u64>());

    let (tx, mut rx) = broadcast::channel::<WSMessage>(500);
    state.ws_sessions.insert(session_id.clone(), tx.clone());

    let mut sensor_rx = state.sensor_broadcast.subscribe();
    let mut alarm_rx = state.alarm_broadcast.subscribe();

    let mut send_task = tokio::spawn(async move {
        loop {
            tokio::select! {
                msg = rx.recv() => {
                    match msg {
                        Ok(ws_msg) => {
                            let text = serde_json::to_string(&ws_msg).unwrap_or_default();
                            if sender.send(Message::Text(text.into())).await.is_err() {
                                break;
                            }
                        }
                        Err(_) => break,
                    }
                }
                sensor = sensor_rx.recv() => {
                    if let Ok(reading) = sensor {
                        let ws_msg = WSMessage::sensor(&reading);
                        let text = serde_json::to_string(&ws_msg).unwrap_or_default();
                        if sender.send(Message::Text(text.into())).await.is_err() {
                            break;
                        }
                    }
                }
                alarm = alarm_rx.recv() => {
                    if let Ok(alarm) = alarm {
                        let ws_msg = WSMessage::alarm(&alarm);
                        let text = serde_json::to_string(&ws_msg).unwrap_or_default();
                        if sender.send(Message::Text(text.into())).await.is_err() {
                            break;
                        }
                    }
                }
            }
        }
    });

    let mut recv_task = tokio::spawn(async move {
        while let Some(msg) = receiver.next().await {
            if let Ok(Message::Text(text)) = msg {
                debug!("收到WS消息: {}", text);
                let _ = tx.send(WSMessage {
                    msg_type: "echo".to_string(),
                    furnace_id: None,
                    data: serde_json::json!({"received": text}),
                    timestamp: Utc::now(),
                });
            }
        }
    });

    tokio::select! {
        _ = &mut send_task => recv_task.abort(),
        _ = &mut recv_task => send_task.abort(),
    }

    state.ws_sessions.remove(&session_id);
    info!("WebSocket连接关闭: {}, 剩余{}个连接", addr, state.ws_sessions.len());
}
