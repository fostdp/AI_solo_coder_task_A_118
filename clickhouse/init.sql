-- 古代风箱鼓风冶铁过程热力学模拟与炉温控制仿真系统
-- ClickHouse 数据库初始化脚本

CREATE DATABASE IF NOT EXISTS metallurgy_simulation
    COMMENT '古代冶金过程仿真数据库'
    ENGINE = Atomic;

USE metallurgy_simulation;

-- 冶炼炉信息表
CREATE TABLE IF NOT EXISTS furnaces (
    furnace_id String COMMENT '炉ID',
    furnace_name String COMMENT '炉名称',
    furnace_type Enum8('Han_Chaogang' = 1, 'Ming_Blast' = 2) COMMENT '炉类型: 汉代炒钢炉/明代高炉',
    volume_m3 Float64 COMMENT '炉容积(m3)',
    max_temperature Float64 COMMENT '最高工作温度(°C)',
    target_temp_min Float64 COMMENT '目标温度下限(°C)',
    target_temp_max Float64 COMMENT '目标温度上限(°C)',
    created_at DateTime DEFAULT now()
)
ENGINE = ReplacingMergeTree()
ORDER BY furnace_id
COMMENT '冶炼炉基础信息表';

-- 传感器实时数据表（核心时序表）
CREATE TABLE IF NOT EXISTS sensor_data (
    timestamp DateTime64(3, 'Asia/Shanghai') DEFAULT now64(3, 'Asia/Shanghai'),
    furnace_id String COMMENT '炉ID',
    push_pull_frequency Float64 COMMENT '风箱推拉频率(次/分钟)',
    stroke_length Float64 COMMENT '风箱行程(cm)',
    wind_pressure Float64 COMMENT '风压(Pa)',
    air_volume FlowRate(Float64) COMMENT '风量(m3/s)',
    furnace_temp Float64 COMMENT '炉内温度(°C)',
    co_concentration Float64 COMMENT 'CO浓度(%)',
    o2_concentration Float64 COMMENT 'O2浓度(%)',
    iron_feed_rate Float64 COMMENT '铁矿进料速率(kg/s)',
    coal_feed_rate Float64 COMMENT '煤炭进料速率(kg/s)',
    pig_iron_output Float64 COMMENT '生铁累计产量(kg)',
    temp_zone_top Float64 COMMENT '炉顶温度(°C)',
    temp_zone_upper Float64 COMMENT '上部温度(°C)',
    temp_zone_middle Float64 COMMENT '中部温度(°C)',
    temp_zone_lower Float64 COMMENT '下部温度(°C)',
    temp_zone_hearth Float64 COMMENT '炉缸温度(°C)',
    reaction_rate Float64 COMMENT '反应速率(mol/s)',
    energy_efficiency Float64 COMMENT '能源效率(%)',
    quality Float64 COMMENT '当前数据质量分数(0-100)',
    protocol Enum8('Modbus_RTU' = 1) DEFAULT 'Modbus_RTU' COMMENT '通信协议'
)
ENGINE = MergeTree()
PARTITION BY toYYYYMM(timestamp)
ORDER BY (furnace_id, timestamp)
TTL toDateTime(timestamp) + INTERVAL 1 YEAR
COMMENT '传感器时序数据表（每10秒上报一次）';

-- 热力学模拟参数表
CREATE TABLE IF NOT EXISTS thermo_simulation_params (
    id UInt64 AUTO_INCREMENT,
    furnace_id String,
    timestamp DateTime64(3) DEFAULT now64(3),
    heat_conductivity Float64 COMMENT '热传导系数(W/(m·K))',
    specific_heat Float64 COMMENT '比热容(J/(kg·K))',
    reaction_enthalpy Float64 COMMENT '反应焓变(J/mol)',
    activation_energy Float64 COMMENT '活化能(J/mol)',
    pre_exponential_factor Float64 COMMENT '指前因子',
    heat_loss_coefficient Float64 COMMENT '热损失系数',
    air_preheat_temp Float64 COMMENT '预热空气温度(°C)'
)
ENGINE = ReplacingMergeTree()
ORDER BY (id, furnace_id, timestamp)
COMMENT '热力学模拟参数配置表';

-- 鼓风优化控制动作表（强化学习）
CREATE TABLE IF NOT EXISTS rl_control_actions (
    timestamp DateTime64(3) DEFAULT now64(3),
    furnace_id String,
    episode UInt32 COMMENT '强化学习回合数',
    step UInt32 COMMENT '当前步数',
    state_vector Array(Float64) COMMENT '状态向量',
    action_frequency Float64 COMMENT '动作:调整后的推拉频率',
    action_stroke Float64 COMMENT '动作:调整后的行程',
    reward Float64 COMMENT '奖励值',
    next_state_vector Array(Float64) COMMENT '下一状态向量',
    done UInt8 COMMENT '是否回合结束',
    loss Float64 COMMENT '模型损失值',
    epsilon Float64 COMMENT '探索率ε',
    learning_rate Float64 COMMENT '学习率'
)
ENGINE = MergeTree()
PARTITION BY toYYYYMM(timestamp)
ORDER BY (furnace_id, episode, step)
COMMENT '强化学习控制动作记录表';

-- 告警事件表
CREATE TABLE IF NOT EXISTS alarm_events (
    event_id UUID DEFAULT generateUUIDv4(),
    timestamp DateTime64(3) DEFAULT now64(3),
    furnace_id String,
    alarm_type Enum8(
        'TEMP_TOO_HIGH' = 1,
        'TEMP_TOO_LOW' = 2,
        'CO_ACCUMULATION' = 3,
        'PRESSURE_ABNORMAL' = 4,
        'EFFICIENCY_LOW' = 5,
        'SYSTEM_ERROR' = 6
    ) COMMENT '告警类型',
    alarm_level Enum8('WARNING' = 1, 'CRITICAL' = 2, 'FATAL' = 3) COMMENT '告警级别',
    message String COMMENT '告警详细信息',
    current_value Float64 COMMENT '当前值',
    threshold_value Float64 COMMENT '阈值',
    acknowledged UInt8 DEFAULT 0 COMMENT '是否已确认',
    mqtt_published UInt8 DEFAULT 0 COMMENT '是否已MQTT推送'
)
ENGINE = ReplacingMergeTree()
ORDER BY (furnace_id, timestamp)
PARTITION BY toYYYYMM(timestamp)
COMMENT '告警事件表';

-- 生铁产量统计表
CREATE TABLE IF NOT EXISTS iron_production_stats (
    stat_date Date COMMENT '统计日期',
    furnace_id String,
    total_iron_kg Float64 COMMENT '当日生铁总产量(kg)',
    total_coal_kg Float64 COMMENT '当日煤炭总消耗(kg)',
    total_iron_ore_kg Float64 COMMENT '当日铁矿总消耗(kg)',
    avg_temp Float64 COMMENT '当日平均温度(°C)',
    avg_co_concentration Float64 COMMENT '当日平均CO浓度(%)',
    avg_energy_efficiency Float64 COMMENT '当日平均能源效率(%)',
    operation_hours Float64 COMMENT '当日运行时长(h)',
    alarm_count UInt32 COMMENT '当日告警次数'
)
ENGINE = SummingMergeTree()
PARTITION BY toYYYYMM(stat_date)
ORDER BY (stat_date, furnace_id)
COMMENT '生铁产量日统计表';

-- 分布式表（集群部署用）
CREATE TABLE IF NOT EXISTS sensor_data_distributed AS sensor_data
ENGINE = Distributed('{cluster}', 'metallurgy_simulation', 'sensor_data', rand());

CREATE TABLE IF NOT EXISTS alarm_events_distributed AS alarm_events
ENGINE = Distributed('{cluster}', 'metallurgy_simulation', 'alarm_events', rand());

-- 初始化基础数据：冶炼炉配置
INSERT INTO furnaces (
    furnace_id, furnace_name, furnace_type, volume_m3, 
    max_temperature, target_temp_min, target_temp_max
) VALUES
(
    'HAN-001', '汉代炒钢炉一号', 'Han_Chaogang', 2.5,
    1450.0, 1200.0, 1350.0
),
(
    'MING-001', '明代高炉一号', 'Ming_Blast', 8.0,
    1600.0, 1350.0, 1500.0
);

-- 初始化热力学参数
INSERT INTO thermo_simulation_params (
    furnace_id, heat_conductivity, specific_heat, reaction_enthalpy,
    activation_energy, pre_exponential_factor, heat_loss_coefficient, air_preheat_temp
) VALUES
(
    'HAN-001', 45.0, 650.0, -824000.0,
    160000.0, 5.0e8, 0.015, 200.0
),
(
    'MING-001', 52.0, 700.0, -850000.0,
    165000.0, 6.5e8, 0.012, 300.0
);

-- 创建物化视图：实时统计告警汇总
CREATE MATERIALIZED VIEW IF NOT EXISTS alarm_summary_mv
ENGINE = SummingMergeTree()
PARTITION BY toYYYYMM(timestamp)
ORDER BY (toStartOfHour(timestamp), furnace_id, alarm_type)
AS
SELECT
    timestamp,
    toStartOfHour(timestamp) AS hour_bucket,
    furnace_id,
    alarm_type,
    alarm_level,
    count() AS total_count,
    sumIf(1, alarm_level = 'CRITICAL') AS critical_count,
    sumIf(1, mqtt_published = 1) AS published_count
FROM alarm_events
GROUP BY timestamp, furnace_id, alarm_type, alarm_level;

-- 创建物化视图：自动计算每日产量统计
CREATE MATERIALIZED VIEW IF NOT EXISTS iron_production_daily_mv
TO iron_production_stats
AS
SELECT
    toDate(timestamp) AS stat_date,
    furnace_id,
    max(pig_iron_output) AS total_iron_kg,
    sum(coal_feed_rate * 10) AS total_coal_kg,
    sum(iron_feed_rate * 10) AS total_iron_ore_kg,
    avg(furnace_temp) AS avg_temp,
    avg(co_concentration) AS avg_co_concentration,
    avg(energy_efficiency) AS avg_energy_efficiency,
    count() * 10 / 3600 AS operation_hours,
    0 AS alarm_count
FROM sensor_data
GROUP BY stat_date, furnace_id;
