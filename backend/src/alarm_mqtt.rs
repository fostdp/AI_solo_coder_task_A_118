use std::collections::{HashMap, VecDeque};
use std::sync::Arc;
use std::time::Duration;

use serde::{Deserialize, Serialize};
use tokio::sync::{mpsc, Mutex};
use tracing::{debug, error, info, warn};

use crate::config::{AlarmConfig, MqttConfigSection, SystemConfig};
use crate::models::{AlarmEvent, AlarmLevel, AlarmType, SensorReading};
use crate::modbus_receiver::ValidatedReading;
use crate::mqtt::{AlarmDetector, AlarmThresholds, MqttPublisher};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum AlarmMqttRequest {
    CheckReading {
        reading: ValidatedReading,
    },
    Acknowledge {
        event_id: String,
        furnace_id: String,
        operator: String,
    },
    GetActiveAlarms,
    GetHistory {
        furnace_id: Option<String>,
        limit: usize,
    },
    SendManual {
        event: AlarmEvent,
    },
    FlushOutbox,
    PublisherStatus,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum AlarmMqttResponse {
    AlarmsDetected {
        events: Vec<AlarmEvent>,
        published_success: usize,
        published_failed: usize,
    },
    Acknowledged {
        event_id: String,
        success: bool,
    },
    ActiveAlarms {
        events: Vec<AlarmEvent>,
    },
    AlarmHistory {
        events: Vec<AlarmEvent>,
    },
    ManualSent {
        event_id: String,
        published: bool,
    },
    OutboxFlushed {
        flushed: usize,
        remaining: usize,
    },
    PublisherStatusInfo {
        connected: bool,
        outbox_size: usize,
        total_published: u64,
        total_failed: u64,
    },
    Error {
        code: String,
        message: String,
    },
}

struct OutboxEntry {
    event: AlarmEvent,
    retry_count: u32,
    next_attempt_at: chrono::DateTime<chrono::Utc>,
}

pub struct AlarmMqttService {
    config: Arc<SystemConfig>,
    mqtt_cfg: MqttConfigSection,
    alarm_cfg: AlarmConfig,
    detector: Mutex<AlarmDetector>,
    thresholds: AlarmThresholds,
    publisher: Mutex<MqttPublisher>,
    active_alarms: Mutex<HashMap<String, AlarmEvent>>,
    history: Mutex<VecDeque<AlarmEvent>>,
    outbox: Mutex<VecDeque<OutboxEntry>>,
    stats: Mutex<AlarmServiceStats>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
struct AlarmServiceStats {
    total_checked: u64,
    total_detected: u64,
    total_published: u64,
    total_failed: u64,
    total_acknowledged: u64,
}

impl AlarmMqttService {
    pub fn new(config: Arc<SystemConfig>) -> Self {
        let thresholds: AlarmThresholds = (&config.alarms).into();
        let mqtt_cfg = config.mqtt.clone();
        let alarm_cfg = config.alarms.clone();

        let mut builder = MqttPublisher::builder();
        builder = builder
            .broker(mqtt_cfg.broker.clone())
            .port(mqtt_cfg.port)
            .topic_prefix(mqtt_cfg.topic_prefix.clone())
            .max_retries(mqtt_cfg.publish_retries);
        if let Some(u) = &mqtt_cfg.username {
            builder = builder.username(u.clone());
        }
        if let Some(p) = &mqtt_cfg.password {
            builder = builder.password(p.clone());
        }
        let publisher = builder.build().unwrap_or_else(|e| {
            error!("MQTT Publisher构建失败: {}", e);
            MqttPublisher::new(&mqtt_cfg.broker, mqtt_cfg.port, &mqtt_cfg.topic_prefix)
        });

        Self {
            config,
            mqtt_cfg,
            alarm_cfg,
            detector: Mutex::new(AlarmDetector::new()),
            thresholds,
            publisher: Mutex::new(publisher),
            active_alarms: Mutex::new(HashMap::new()),
            history: Mutex::new(VecDeque::with_capacity(5000)),
            outbox: Mutex::new(VecDeque::with_capacity(1000)),
            stats: Mutex::new(AlarmServiceStats::default()),
        }
    }

    pub async fn start(
        self,
        mut reading_rx: mpsc::Receiver<ValidatedReading>,
        mut req_rx: mpsc::Receiver<AlarmMqttRequest>,
        resp_tx: mpsc::Sender<AlarmMqttResponse>,
        broadcast_tx: tokio::sync::broadcast::Sender<AlarmEvent>,
    ) {
        let retry_interval = Duration::from_secs(self.mqtt_cfg.outbox_retry_interval_secs);
        info!(
            "AlarmMqttService 启动, broker={}:{}, topic={}, outbox重试={:?}",
            self.mqtt_cfg.broker,
            self.mqtt_cfg.port,
            self.mqtt_cfg.topic_prefix,
            retry_interval
        );

        let self_arc = Arc::new(self);
        let service_ref = self_arc.clone();

        let read_service = self_arc.clone();
        let broadcast = broadcast_tx.clone();
        tokio::spawn(async move {
            while let Some(validated) = reading_rx.recv().await {
                read_service
                    .process_validated(validated, &broadcast_tx.clone())
                    .await;
            }
            info!("Reading处理worker退出");
        });

        let outbox_service = service_ref.clone();
        tokio::spawn(async move {
            loop {
                tokio::time::sleep(retry_interval).await;
                outbox_service.flush_outbox_once(&broadcast_tx.clone()).await;
            }
        });

        while let Some(req) = req_rx.recv().await {
            let resp = service_ref.handle_request(req, &broadcast_tx).await;
            if let Err(e) = resp_tx.send(resp).await {
                error!("发送AlarmMqtt响应失败: {}", e);
                break;
            }
        }
        info!("AlarmMqttService 退出");
    }

    async fn process_validated(
        &self,
        validated: ValidatedReading,
        broadcast: &tokio::sync::broadcast::Sender<AlarmEvent>,
    ) {
        let reading = validated.reading;
        {
            let mut st = self.stats.lock().await;
            st.total_checked += 1;
        }

        let events: Vec<AlarmEvent>;
        {
            let mut detector = self.detector.lock().await;
            events = detector.detect(&reading, &self.thresholds);
        }

        if events.is_empty() {
            return;
        }

        {
            let mut st = self.stats.lock().await;
            st.total_detected += events.len() as u64;
        }

        let mut success = 0;
        let mut fail = 0;

        for event in events {
            let atype = format!("{:?}", event.alarm_type);
            let alevel = format!("{:?}", event.alarm_level);
            crate::metrics::inc_alarm(&event.furnace_id, &atype, &alevel);
            self.history.lock().await.push_back(event.clone());
            {
                let mut hist = self.history.lock().await;
                if hist.len() > 5000 {
                    hist.drain(0..hist.len() - 5000);
                }
            }
            if !event.acknowledged {
                self.active_alarms
                    .lock()
                    .await
                    .insert(event.event_id.clone(), event.clone());
            }

            let published = self.publish_event(&event).await;
            if published {
                success += 1;
            } else {
                fail += 1;
                self.enqueue_outbox(event.clone()).await;
            }

            let _ = broadcast.send(event);
        }

        let active_count = self.active_alarms
            .lock()
            .await
            .values()
            .filter(|e| !e.acknowledged)
            .count();
        crate::metrics::set_active_alarms(active_count as f64);

        debug!(
            "告警检测: 命中{}, MQTT成功{}, 失败{}",
            success + fail,
            success,
            fail
        );
    }

    async fn publish_event(&self, event: &AlarmEvent) -> bool {
        let publisher = self.publisher.lock().await;
        match publisher.publish_alarm(event).await {
            Ok(_) => {
                self.stats.lock().await.total_published += 1;
                crate::metrics::inc_mqtt_publish_ok();
                true
            }
            Err(e) => {
                warn!("MQTT推送告警失败: {} (event={})", e, event.event_id);
                self.stats.lock().await.total_failed += 1;
                crate::metrics::inc_mqtt_publish_err();
                false
            }
        }
    }

    async fn enqueue_outbox(&self, event: AlarmEvent) {
        let next_attempt = chrono::Utc::now()
            + chrono::Duration::seconds(self.mqtt_cfg.outbox_retry_interval_secs as i64);
        self.outbox.lock().await.push_back(OutboxEntry {
            event,
            retry_count: 0,
            next_attempt_at: next_attempt,
        });
    }

    async fn flush_outbox_once(
        &self,
        broadcast: &tokio::sync::broadcast::Sender<AlarmEvent>,
    ) {
        let now = chrono::Utc::now();
        let mut ready = Vec::new();

        {
            let mut outbox = self.outbox.lock().await;
            let mut i = 0;
            while i < outbox.len() {
                if outbox[i].next_attempt_at <= now {
                    ready.push(outbox.remove(i));
                } else {
                    i += 1;
                }
            }
        }

        for entry in ready {
            if entry.retry_count >= self.mqtt_cfg.publish_retries {
                error!(
                    "Outbox告警 {} 重试{}次仍失败，丢弃",
                    entry.event.event_id, entry.retry_count
                );
                continue;
            }

            let ok = self.publish_event(&entry.event).await;
            if !ok {
                let next = chrono::Utc::now()
                    + chrono::Duration::seconds(
                        (self.mqtt_cfg.outbox_retry_interval_secs
                            * (entry.retry_count + 1) as u64) as i64,
                    );
                let entry_back = OutboxEntry {
                    event: entry.event,
                    retry_count: entry.retry_count + 1,
                    next_attempt_at: next,
                };
                self.outbox.lock().await.push_back(entry_back);
            } else {
                let _ = broadcast.send(entry.event);
            }
        }
    }

    async fn handle_request(
        &self,
        req: AlarmMqttRequest,
        broadcast: &tokio::sync::broadcast::Sender<AlarmEvent>,
    ) -> AlarmMqttResponse {
        match req {
            AlarmMqttRequest::CheckReading { reading } => {
                self.process_validated(reading, broadcast).await;
                AlarmMqttResponse::AlarmsDetected {
                    events: Vec::new(),
                    published_success: 0,
                    published_failed: 0,
                }
            }
            AlarmMqttRequest::Acknowledge {
                event_id,
                furnace_id,
                operator,
            } => {
                let mut active = self.active_alarms.lock().await;
                let ev = active.get_mut(&event_id);
                let ok = match ev {
                    Some(e) => {
                        e.acknowledged = true;
                        e.acknowledged_by = Some(operator);
                        e.acknowledged_at = Some(chrono::Utc::now());
                        true
                    }
                    None => {
                        let mut hist = self.history.lock().await;
                        match hist.iter_mut().find(|h| h.event_id == event_id) {
                            Some(e) => {
                                e.acknowledged = true;
                                e.acknowledged_by = Some(operator);
                                e.acknowledged_at = Some(chrono::Utc::now());
                                true
                            }
                            None => false,
                        }
                    }
                };
                if ok {
                    self.stats.lock().await.total_acknowledged += 1;
                }
                AlarmMqttResponse::Acknowledged {
                    event_id,
                    success: ok,
                }
            }
            AlarmMqttRequest::GetActiveAlarms => {
                let events: Vec<AlarmEvent> = self
                    .active_alarms
                    .lock()
                    .await
                    .values()
                    .filter(|e| !e.acknowledged)
                    .cloned()
                    .collect();
                AlarmMqttResponse::ActiveAlarms { events }
            }
            AlarmMqttRequest::GetHistory {
                furnace_id,
                limit,
            } => {
                let hist = self.history.lock().await;
                let mut events: Vec<AlarmEvent> = match furnace_id {
                    Some(id) => hist.iter().filter(|e| e.furnace_id == id).cloned().collect(),
                    None => hist.iter().cloned().collect(),
                };
                events.reverse();
                events.truncate(limit.max(1));
                AlarmMqttResponse::AlarmHistory { events }
            }
            AlarmMqttRequest::SendManual { event } => {
                let published = self.publish_event(&event).await;
                if !published {
                    self.enqueue_outbox(event.clone()).await;
                }
                let _ = broadcast.send(event.clone());
                self.history.lock().await.push_back(event.clone());
                AlarmMqttResponse::ManualSent {
                    event_id: event.event_id,
                    published,
                }
            }
            AlarmMqttRequest::FlushOutbox => {
                let before = self.outbox.lock().await.len();
                self.flush_outbox_once(broadcast).await;
                let after = self.outbox.lock().await.len();
                AlarmMqttResponse::OutboxFlushed {
                    flushed: before - after,
                    remaining: after,
                }
            }
            AlarmMqttRequest::PublisherStatus => {
                let st = self.stats.lock().await;
                AlarmMqttResponse::PublisherStatusInfo {
                    connected: true,
                    outbox_size: self.outbox.try_lock().map(|o| o.len()).unwrap_or(0),
                    total_published: st.total_published,
                    total_failed: st.total_failed,
                }
            }
        }
    }
}
