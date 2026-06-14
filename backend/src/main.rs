use std::sync::Arc;
use std::time::Duration;

use anyhow::{Context, Result};
use clap::Parser;
use tokio::net::TcpListener;
use tower_http::cors::{Any, CorsLayer};
use tower_http::trace::TraceLayer;
use tracing::{error, info, warn};
use tracing_subscriber::EnvFilter;

use metallurgy_simulation::*;
use metallurgy_simulation::api::AppState;
use metallurgy_simulation::models::{FurnaceConfig, FurnaceType, ThermoParams};
use metallurgy_simulation::parameter_id::MultiFurnaceIdentifier;
use metallurgy_simulation::qlearning::MultiFurnaceQLController;
use metallurgy_simulation::thermodynamics::MultiFurnaceThermoEngine;
use metallurgy_simulation::rl_control::MultiFurnaceRLController;
use metallurgy_simulation::mqtt::{AlarmDetector, MqttConfig, MqttPublisher};
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

    info!("启动冶金过程仿真服务...");
    info!("  监听地址: {}:{}", args.host, args.port);
    info!("  ClickHouse: {} / {}", args.clickhouse_url, args.clickhouse_db);
    info!("  MQTT Broker: {}:{}", args.mqtt_broker, args.mqtt_port);

    let store = init_storage(&args).await?;
    let mut thermo_engine = MultiFurnaceThermoEngine::new();
    let mut rl_controller = MultiFurnaceRLController::new();
    let mut ql_controller = MultiFurnaceQLController::new();
    let mut param_identifier = MultiFurnaceIdentifier::new();
    let mut alarm_detector = AlarmDetector::new();

    for cfg in FURNACE_CONFIGS {
        let furnace_config = FurnaceConfig {
            furnace_id: cfg.0.to_string(),
            furnace_name: cfg.1.to_string(),
            furnace_type: cfg.2,
            volume_m3: cfg.3,
            max_temperature: cfg.4,
            target_temp_min: cfg.5,
            target_temp_max: cfg.6,
        };

        let thermo_params = ThermoParams {
            furnace_id: cfg.0.to_string(),
            heat_conductivity: cfg.7,
            specific_heat: cfg.8,
            reaction_enthalpy: cfg.9,
            activation_energy: cfg.10,
            pre_exponential_factor: cfg.11,
            heat_loss_coefficient: cfg.12,
            air_preheat_temp: cfg.13,
        };

        thermo_engine.add_furnace(furnace_config.clone(), thermo_params);
        rl_controller.add_furnace(cfg.0.to_string());
        ql_controller.add_furnace(cfg.0.to_string(), furnace_config);
        param_identifier.add_furnace(cfg.0.to_string(), (cfg.10, cfg.11, cfg.12));

        info!("  [初始化] {} ({}) - 目标温度: {:.0}-{:.0}°C",
            cfg.1, cfg.0, cfg.5, cfg.6);
    }

    let mqtt_config = MqttConfig {
        broker_url: args.mqtt_broker.clone(),
        port: args.mqtt_port,
        client_id: format!("metallurgy_backend_{}", std::process::id()),
        username: args.mqtt_username.clone(),
        password: args.mqtt_password.clone(),
        topic_prefix: args.mqtt_topic_prefix.clone(),
        keep_alive: 60,
    };

    let mut mqtt_publisher = MqttPublisher::new(mqtt_config.clone());
    match mqtt_publisher.connect().await {
        Ok(_) => info!("MQTT Publisher 连接成功"),
        Err(e) => {
            warn!("MQTT Publisher 连接失败 (将以离线模式运行): {}", e);
        }
    }

    let app_state = Arc::new(AppState::new(
        store.clone(),
        thermo_engine,
        rl_controller,
        ql_controller,
        param_identifier,
        alarm_detector,
        mqtt_publisher,
    ));

    let mqtt_cfg_clone = mqtt_config.clone();
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

    let listen_addr = format!("{}:{}", args.host, args.port);
    info!("HTTP/WebSocket服务启动于: http://{}", listen_addr);
    println!("\n✅ 系统启动完成！");
    println!("   API文档:");
    println!("     GET  /api/health                    - 健康检查");
    println!("     GET  /api/status                    - 系统状态");
    println!("     GET  /api/furnaces/                 - 冶炼炉列表");
    println!("     POST /api/sensor/report             - 传感器数据上报");
    println!("     GET  /api/furnaces/:id/temp_field   - 温度云图");
    println!("     GET  /api/alarms/                   - 告警列表");
    println!("     GET  /api/ql/status                 - Q-Learning训练状态(默认)");
    println!("     GET  /api/rl/status                 - DDPG训练状态(兼容)");
    println!("     GET  /api/param_id/status           - 参数辨识状态");
    println!("     PUT  /api/ql/algo                   - 切换控制算法");
    println!("     WS   /ws                            - WebSocket实时推送");
    println!();

    let listener = TcpListener::bind(&listen_addr)
        .await
        .with_context(|| format!("无法绑定监听地址: {}", listen_addr))?;

    axum::serve(listener, router)
        .await
        .with_context(|| "Axum服务启动失败")?;

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

async fn init_storage(args: &CliArgs) -> Result<ClickHouseStore> {
    let store = ClickHouseStore::new(
        &args.clickhouse_url,
        &args.clickhouse_db,
        &args.clickhouse_user,
        &args.clickhouse_password,
    )?;

    if !args.skip_db_check {
        info!("检查ClickHouse连接...");
        match store.ping().await {
            Ok(true) => info!("ClickHouse连接正常"),
            Ok(false) => warn!("ClickHouse返回异常"),
            Err(e) => {
                error!("ClickHouse连接失败: {}", e);
                if !args.auto_init {
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
