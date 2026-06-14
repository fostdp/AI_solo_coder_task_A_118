use std::sync::Arc;
use std::time::Duration;

use anyhow::{Context, Result};
use clap::Parser;
use tokio::net::TcpListener;
use tokio::sync::{broadcast, mpsc};
use tower_http::cors::{Any, CorsLayer};
use tower_http::trace::TraceLayer;
use tracing::{error, info, warn};
use tracing_subscriber::{EnvFilter, FmtSubscriber};

use metallurgy_simulation::*;
use metallurgy_simulation::alarm_mqtt::{AlarmMqttRequest, AlarmMqttResponse, AlarmMqttService};
use metallurgy_simulation::api::AppState;
use metallurgy_simulation::config::{ControlAlgorithm, SystemConfig};
use metallurgy_simulation::control_optimizer::{
    ControlOptimizer, ControlRequest, ControlResponse,
};
use metallurgy_simulation::models::{AlarmEvent, FurnaceConfig, FurnaceType, SensorReading, ThermoParams};
use metallurgy_simulation::modbus_receiver::{ModbusReceiver, ValidatedReading};
use metallurgy_simulation::mqtt::{AlarmDetector, MqttConfig, MqttPublisher};
use metallurgy_simulation::parameter_id::MultiFurnaceIdentifier;
use metallurgy_simulation::qlearning::MultiFurnaceQLController;
use metallurgy_simulation::thermodynamics_simulator::{
    update_cached_reading, ThermodynamicsSimulator, ThermoRequest, ThermoResponse,
};
use metallurgy_simulation::thermodynamics::MultiFurnaceThermoEngine;
use metallurgy_simulation::rl_control::MultiFurnaceRLController;
use metallurgy_simulation::storage::ClickHouseStore;

#[derive(Parser, Debug, Clone)]
#[command(
    name = "metallurgy-simulation-server",
    about = "古代风箱鼓风冶铁过程热力学模拟与炉温控制仿真系统后端服务",
    version = "0.1.0"
)]
struct CliArgs {
    #[arg(long, default_value = "0.0.0.0", env = "SERVER_HOST")]
    host: String,

    #[arg(long, default_value_t = 8080, env = "SERVER_PORT")]
    port: u16,

    #[arg(long, default_value = "http://127.0.0.1:8123", env = "CLICKHOUSE_URL")]
    clickhouse_url: String,

    #[arg(long, default_value = "metallurgy_simulation", env = "CLICKHOUSE_DB")]
    clickhouse_db: String,

    #[arg(long, default_value = "default", env = "CLICKHOUSE_USER")]
    clickhouse_user: String,

    #[arg(long, default_value = "", env = "CLICKHOUSE_PASSWORD")]
    clickhouse_password: String,

    #[arg(long, default_value = "127.0.0.1", env = "MQTT_BROKER")]
    mqtt_broker: String,

    #[arg(long, default_value_t = 1883, env = "MQTT_PORT")]
    mqtt_port: u16,

    #[arg(long, env = "MQTT_USERNAME")]
    mqtt_username: Option<String>,

    #[arg(long, env = "MQTT_PASSWORD")]
    mqtt_password: Option<String>,

    #[arg(long, default_value = "metallurgy/alarms", env = "MQTT_TOPIC_PREFIX")]
    mqtt_topic_prefix: String,

    #[arg(long, default_value = "info", env = "LOG_LEVEL")]
    log_level: String,

    #[arg(long, default_value_t = false)]
    skip_db_check: bool,

    #[arg(long, default_value_t = true)]
    auto_init: bool,
}

const FURNACE_CONFIGS: &[(&str, &str, FurnaceType, f64, f64, f64, f64, f64, f64, f64, f64, f64, f64, f64)] = &[
    (
        "HAN-001", "汉代炒钢炉一号", FurnaceType::HanChaogang,
        2.5, 1450.0, 1200.0, 1350.0,
        45.0, 650.0, -824000.0, 160000.0, 5.0e8, 0.015, 200.0,
    ),
    (
        "MING-001", "明代高炉一号", FurnaceType::MingBlast,
        8.0, 1600.0, 1350.0, 1500.0,
        52.0, 700.0, -850000.0, 165000.0, 6.5e8, 0.012, 300.0,
    ),
];

#[tokio::main]
async fn main() -> Result<()> {
    let args = CliArgs::parse();
    init_logging(&args.log_level);

    println_banner();

    let mut sys_config = SystemConfig::from_env();
    sys_config.server.host = args.host.clone();
    sys_config.server.port = args.port;
    sys_config.clickhouse.url = args.clickhouse_url.clone();
    sys_config.clickhouse.database = args.clickhouse_db.clone();
    sys_config.clickhouse.username = args.clickhouse_user.clone();
    sys_config.clickhouse.password = args.clickhouse_password.clone();
    sys_config.clickhouse.skip_check = args.skip_db_check;
    sys_config.clickhouse.auto_init = args.auto_init;
    sys_config.mqtt.broker = args.mqtt_broker.clone();
    sys_config.mqtt.port = args.mqtt_port;
    sys_config.mqtt.username = args.mqtt_username.clone();
    sys_config.mqtt.password = args.mqtt_password.clone();
    sys_config.mqtt.topic_prefix = args.mqtt_topic_prefix.clone();

    let config = Arc::new(sys_config);

    info!("启动冶金过程仿真服务...");
    info!("  监听地址: {}:{}", config.server.host, config.server.port);
    info!("  ClickHouse: {} / {}", config.clickhouse.url, config.clickhouse.database);
    info!("  MQTT Broker: {}:{}", config.mqtt.broker, config.mqtt.port);
    info!("  控制算法默认: {:?}", config.control.default_algo);

    let furnace_cfgs = config.furnace_configs();
    for (fc, tp) in &furnace_cfgs {
        info!(
            "  [初始化] {} ({}) - 目标温度: {:.0}-{:.0}°C",
            fc.furnace_name,
            fc.furnace_id,
            fc.target_temp_min,
            fc.target_temp_max
        );
    }

    let store = init_storage(&config).await?;

    let ch_cfg = config.channels.clone();
    let (sensor_tx, sensor_rx) = mpsc::channel::<SensorReading>(ch_cfg.sensor_rx_buffer);
    let (validated_tx, validated_rx) = mpsc::channel::<ValidatedReading>(ch_cfg.sensor_rx_buffer);
    let (post_thermo_tx, post_thermo_rx) = mpsc::channel::<ValidatedReading>(ch_cfg.thermo_rx_buffer);
    let (thermo_req_tx, thermo_req_rx) = mpsc::channel::<ThermoRequest>(32);
    let (thermo_resp_tx, _thermo_resp_rx) = mpsc::channel::<ThermoResponse>(32);
    let (control_validated_tx, control_validated_rx) = mpsc::channel::<ValidatedReading>(ch_cfg.control_rx_buffer);
    let (control_req_tx, control_req_rx) = mpsc::channel::<ControlRequest>(32);
    let (control_resp_tx, _control_resp_rx) = mpsc::channel::<ControlResponse>(32);
    let (post_control_tx, post_control_rx) = mpsc::channel::<ControlResponse>(ch_cfg.action_broadcast);
    let (alarm_validated_tx, alarm_validated_rx) = mpsc::channel::<ValidatedReading>(ch_cfg.alarm_rx_buffer);
    let (alarm_req_tx, alarm_req_rx) = mpsc::channel::<AlarmMqttRequest>(32);
    let (alarm_resp_tx, _alarm_resp_rx) = mpsc::channel::<AlarmMqttResponse>(32);
    let (sensor_broadcast, _) = broadcast::channel::<SensorReading>(ch_cfg.action_broadcast);
    let (alarm_broadcast, _) = broadcast::channel::<AlarmEvent>(ch_cfg.alarm_rx_buffer);
    let (action_broadcast, _) = broadcast::channel::<ControlResponse>(ch_cfg.action_broadcast);

    let receiver = ModbusReceiver::new(config.clone());
    let receiver_task = tokio::spawn(async move {
        receiver.start(sensor_rx, validated_tx, None).await;
    });

    let furnace_configs: Vec<(FurnaceConfig, ThermoParams)> = furnace_cfgs.clone();
    let thermo_sim = ThermodynamicsSimulator::new(config.clone(), furnace_configs.clone());
    let thermo_task = tokio::spawn(async move {
        thermo_sim
            .start(thermo_req_rx, thermo_resp_tx, validated_rx, post_thermo_tx)
            .await;
    });

    let post_thermo_to_control = post_thermo_tx.clone();
    let cvt_control = control_validated_tx.clone();
    let cvt_alarm = alarm_validated_tx.clone();
    let cache_updater_sensor = sensor_broadcast.clone();
    tokio::spawn(async move {
        let mut post_rx = post_thermo_rx;
        while let Some(v) = post_rx.recv().await {
            update_cached_reading(&v.reading.furnace_id, &v.reading);
            let _ = cache_updater_sensor.send(v.reading.clone());
            let _ = cvt_control.send(v.clone()).await;
            let _ = cvt_alarm.send(v).await;
        }
    });

    let furnaces_only: Vec<FurnaceConfig> = furnace_configs.iter().map(|(f, _)| f.clone()).collect();
    let ctrl_opt = ControlOptimizer::new(config.clone(), furnaces_only);
    let control_task = tokio::spawn(async move {
        ctrl_opt
            .start(
                control_validated_rx,
                control_req_rx,
                control_resp_tx,
                post_control_tx,
            )
            .await;
    });

    let post_action_broadcast = action_broadcast.clone();
    tokio::spawn(async move {
        let mut post_rx = post_control_rx;
        while let Some(resp) = post_rx.recv().await {
            let _ = post_action_broadcast.send(resp);
        }
    });

    let alarm_service = AlarmMqttService::new(config.clone());
    let alarm_task = tokio::spawn(async move {
        alarm_service
            .start(
                alarm_validated_rx,
                alarm_req_rx,
                alarm_resp_tx,
                alarm_broadcast.clone(),
            )
            .await;
    });

    let backward_thermo_engine = MultiFurnaceThermoEngine::new();
    let backward_ql = MultiFurnaceQLController::new();
    let backward_rl = MultiFurnaceRLController::new();
    let backward_pid = MultiFurnaceIdentifier::new();
    let backward_detector = AlarmDetector::new();

    let basic_fc: Vec<(FurnaceConfig, ThermoParams)> = furnace_cfgs.clone();
    let mut backward_thermo_engine = backward_thermo_engine;
    let mut backward_ql = backward_ql;
    let mut backward_pid = backward_pid;
    for (fc, tp) in &basic_fc {
        backward_thermo_engine.add_furnace(fc.clone(), tp.clone());
        backward_ql.add_furnace(fc.furnace_id.clone(), fc.clone());
        backward_rl.add_furnace(fc.furnace_id.clone());
        backward_pid.add_furnace(
            fc.furnace_id.clone(),
            (tp.activation_energy, tp.pre_exponential_factor, tp.heat_loss_coefficient),
        );
    }

    let basic_mqtt_cfg = MqttConfig {
        broker_url: config.mqtt.broker.clone(),
        port: config.mqtt.port,
        client_id: format!("metallurgy_backend_{}", std::process::id()),
        username: config.mqtt.username.clone(),
        password: config.mqtt.password.clone(),
        topic_prefix: config.mqtt.topic_prefix.clone(),
        keep_alive: config.mqtt.keep_alive_secs as u64,
    };

    let mut backward_publisher = MqttPublisher::new(basic_mqtt_cfg.clone());
    match backward_publisher.connect().await {
        Ok(_) => info!("MQTT Publisher 连接成功"),
        Err(e) => {
            warn!("MQTT Publisher 连接失败 (将以离线模式运行): {}", e);
        }
    }

    let store_to_api = store.clone();
    let api_thermo_read = tokio::sync::RwLock::new(backward_thermo_engine);
    let api_ql_read = tokio::sync::RwLock::new(backward_ql);
    let api_pid_read = tokio::sync::RwLock::new(backward_pid);
    let api_algo = tokio::sync::RwLock::new(match config.control.default_algo {
        ControlAlgorithm::QLearning => api::ControlAlgo::QLearning,
        ControlAlgorithm::Ddpg => api::ControlAlgo::Ddpg,
    });
    let backward_detector = tokio::sync::Mutex::new(backward_detector);

    let mut state = AppState::new(
        store_to_api,
        backward_thermo_engine,
        backward_rl,
        backward_ql,
        backward_pid,
        backward_detector,
        backward_publisher,
    );

    state.inject_channels(
        sensor_tx,
        thermo_req_tx,
        control_req_tx,
        alarm_req_tx,
        sensor_broadcast,
        alarm_broadcast,
        action_broadcast,
        api_thermo_read,
        api_ql_read,
        api_pid_read,
        api_algo,
    );

    let app_state = Arc::new(state);

    let mqtt_cfg_clone = basic_mqtt_cfg.clone();
    let app_state_clone = app_state.clone();
    tokio::spawn(async move {
        start_mqtt_subscriber(mqtt_cfg_clone, app_state_clone).await;
    });

    let router = metallurgy_simulation::api::build_router(app_state.clone())
        .layer(
            CorsLayer::new()
                .allow_origin(Any)
                .allow_methods(Any)
                .allow_headers(Any),
        )
        .layer(TraceLayer::new_for_http())
        .with_state(app_state.clone());

    let listen_addr = format!("{}:{}", config.server.host, config.server.port);
    info!("HTTP/WebSocket服务启动于: http://{}", listen_addr);
    println!("\n✅ 系统启动完成 (模块化版本)！");
    println!("   Actor通道拓扑:");
    println!("     Sensor → ModbusReceiver → ThermodynamicsSimulator → ControlOptimizer");
    println!("                                              ↘ AlarmMqttService → MQTT + WS广播");
    println!("   API文档:");
    println!("     GET  /api/health                    - 健康检查");
    println!("     GET  /api/status                    - 系统状态");
    println!("     GET  /api/furnaces/                 - 冶炼炉列表");
    println!("     POST /api/sensor/report             - 传感器数据上报");
    println!("     GET  /api/furnaces/:id/temp_field   - 温度云图");
    println!("     GET  /api/alarms/                   - 告警列表");
    println!("     GET  /api/ql/status                 - Q-Learning训练状态");
    println!("     GET  /api/rl/status                 - DDPG训练状态(兼容)");
    println!("     GET  /api/param_id/status           - 参数辨识状态");
    println!("     PUT  /api/ql/algo                   - 切换控制算法");
    println!("     WS   /ws                            - WebSocket实时推送");
    println!();

    let listener = TcpListener::bind(&listen_addr)
        .await
        .with_context(|| format!("无法绑定监听地址: {}", listen_addr))?;

    let server_task = axum::serve(listener, router);
    let result = tokio::select! {
        r = server_task => r.map_err(|e| anyhow::anyhow!("Axum服务失败: {}", e)),
        _ = receiver_task => { warn!("ModbusReceiver已退出"); Ok(()) },
        _ = thermo_task => { warn!("ThermodynamicsSimulator已退出"); Ok(()) },
        _ = control_task => { warn!("ControlOptimizer已退出"); Ok(()) },
        _ = alarm_task => { warn!("AlarmMqttService已退出"); Ok(()) },
    };

    if let Err(e) = result {
        error!("系统异常退出: {}", e);
    }

    Ok(())
}

fn init_logging(level: &str) {
    let env_filter = EnvFilter::try_from_default_env()
        .unwrap_or_else(|_| EnvFilter::new(level));

    tracing_subscriber::fmt()
        .with_env_filter(env_filter)
        .with_target(true)
        .with_thread_ids(true)
        .with_line_number(true)
        .with_file(false)
        .with_level(true)
        .with_ansi(true)
        .compact()
        .init();
}

async fn init_storage(config: &SystemConfig) -> Result<ClickHouseStore> {
    let store = ClickHouseStore::new(
        &config.clickhouse.url,
        &config.clickhouse.database,
        &config.clickhouse.username,
        &config.clickhouse.password,
    )?;

    if !config.clickhouse.skip_check {
        info!("检查ClickHouse连接...");
        match store.ping().await {
            Ok(true) => info!("ClickHouse连接正常"),
            Ok(false) => warn!("ClickHouse返回异常"),
            Err(e) => {
                error!("ClickHouse连接失败: {}", e);
                if !config.clickhouse.auto_init {
                    anyhow::bail!("ClickHouse连接失败: {}", e);
                }
                warn!("继续运行（数据将无法持久化）");
            }
        }
    }

    Ok(store)
}

async fn start_mqtt_subscriber(config: MqttConfig, _state: Arc<AppState>) {
    use rumqttc::{AsyncClient, MqttOptions, Event, Packet, QoS};

    let mut opts = MqttOptions::new(
        format!("{}-sub", config.client_id),
        &config.broker_url,
        config.port,
    );
    opts.set_keep_alive(Duration::from_secs(config.keep_alive));

    if let Some(username) = &config.username {
        opts.set_credentials(username, config.password.clone().unwrap_or_default());
    }

    let topic_ack = format!("{}/+/+/ack", config.topic_prefix);
    let topic_cmd = format!("{}/+/command", config.topic_prefix);

    match AsyncClient::new(opts, 100) {
        (client, mut eventloop) => {
            if let Err(e) = client.subscribe(&topic_ack, QoS::AtLeastOnce).await {
                warn!("MQTT订阅失败 ({}): {}", topic_ack, e);
            }
            if let Err(e) = client.subscribe(&topic_cmd, QoS::AtLeastOnce).await {
                warn!("MQTT订阅失败 ({}): {}", topic_cmd, e);
            }

            loop {
                match eventloop.poll().await {
                    Ok(Event::Incoming(Packet::Publish(p))) => {
                        info!("收到MQTT消息: topic={}", p.topic);
                    }
                    Ok(_) => {}
                    Err(e) => {
                        warn!("MQTT订阅eventloop错误: {}", e);
                        tokio::time::sleep(Duration::from_secs(5)).await;
                    }
                }
            }
        }
        Err(e) => {
            error!("MQTT订阅客户端创建失败: {}", e);
        }
    }
}

fn println_banner() {
    let banner = r#"
╔══════════════════════════════════════════════════════════════════╗
║                                                                  ║
║     古代风箱鼓风冶铁过程热力学模拟与炉温控制仿真系统             ║
║     Metallurgy Bellows Simulation & Furnace Temp Control        ║
║                                                                  ║
║     汉代炒钢炉 (HAN) · 明代高炉 (MING)                           ║
║     Modbus RTU · ClickHouse · DDPG-RL · MQTT · Three.js         ║
║                                                                  ║
╚══════════════════════════════════════════════════════════════════╝
"#;
    println!("{}", banner);
}
