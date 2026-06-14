import { WSClient, ApiClient } from './client.js';
import { Furnace3D } from './furnace3d.js';
import { BellowsAnimation } from './bellows.js';
import { TempFieldVisualization } from './tempField.js';
import { TemperatureChart } from './tempChart.js';
import { ControlPanel } from './control_panel.js';

const CONFIG = {
    API_BASE_URL: (location.origin.includes('localhost') || location.hostname === '127.0.0.1')
        ? 'http://127.0.0.1:8080'
        : location.origin,
    AUTO_REFRESH_INTERVAL: 5000,
};

class MetallurgyApp {
    constructor() {
        this.currentFurnace = 'HAN-001';
        this.furnaceConfigs = {};
        this.alarms = [];
        this.autoControlEnabled = true;
        this.lastReading = null;
        this.rlStatus = null;
        this.recommendedAction = null;

        this.api = new ApiClient(CONFIG.API_BASE_URL);
        this.ws = null;

        this.init();
    }

    async init() {
        this.setupEventListeners();
        this.initVisualizations();
        this.initControlPanel();
        this.updateCurrentTime();
        setInterval(() => this.updateCurrentTime(), 1000);

        await this.loadFurnaceConfigs();
        if (this.controlPanel) {
            this.controlPanel.setFurnaceList(Object.values(this.furnaceConfigs));
        }
        this.initWebSocket();
        this.loadInitialData();
        if (this.controlPanel) {
            setTimeout(() => this.controlPanel.refreshStatus(), 800);
            setInterval(() => this.controlPanel.refreshStatus(), 15000);
        }

        setInterval(() => {
            if (!this.ws || !this.ws.isConnected) {
                this.loadInitialData();
            }
        }, CONFIG.AUTO_REFRESH_INTERVAL);
    }

    setupEventListeners() {
        const selector = document.getElementById('furnaceSelector');
        if (selector) {
            selector.addEventListener('change', (e) => {
                this.switchFurnace(e.target.value);
            });
        }

        const sliderFreq = document.getElementById('sliderFreq');
        const sliderStroke = document.getElementById('sliderStroke');
        const freqVal = document.getElementById('sliderFreqVal');
        const strokeVal = document.getElementById('sliderStrokeVal');

        if (sliderFreq) {
            sliderFreq.addEventListener('input', (e) => {
                freqVal.textContent = e.target.value;
            });
        }
        if (sliderStroke) {
            sliderStroke.addEventListener('input', (e) => {
                strokeVal.textContent = e.target.value;
            });
        }

        const btnApply = document.getElementById('btnApplyControl');
        if (btnApply) {
            btnApply.addEventListener('click', () => this.applyManualControl());
        }

        const btnAuto = document.getElementById('btnAutoControl');
        if (btnAuto) {
            btnAuto.addEventListener('click', () => this.toggleAutoControl());
        }
    }

    initVisualizations() {
        try {
            this.furnace3d = new Furnace3D('furnace3d');
            console.log('[App] Furnace3D 初始化完成');
        } catch (e) {
            console.error('[App] Furnace3D 初始化失败:', e);
        }

        try {
            this.bellows = new BellowsAnimation('bellowsCanvas');
            console.log('[App] Bellows 动画初始化完成');
        } catch (e) {
            console.error('[App] Bellows 动画初始化失败:', e);
        }

        try {
            this.tempField = new TempFieldVisualization('tempFieldCanvas');
            console.log('[App] 温度云图初始化完成');
        } catch (e) {
            console.error('[App] 温度云图初始化失败:', e);
        }

        try {
            this.tempChart = new TemperatureChart('tempChartCanvas');
            console.log('[App] 温度图表初始化完成');
        } catch (e) {
            console.error('[App] 温度图表初始化失败:', e);
        }
    }

    initControlPanel() {
        try {
            this.controlPanel = new ControlPanel({
                containerId: 'controlPanel',
                apiBaseUrl: CONFIG.API_BASE_URL,
                initialFurnaceId: this.currentFurnaceId,
                furnaces: Object.values(this.furnaceConfigs),
                initialMode: 'auto',
            });

            this.controlPanel.on('furnaceChange', (id) => {
                this.switchFurnace(id);
                const topSel = document.getElementById('furnaceSelector');
                if (topSel) topSel.value = id;
            });
            this.controlPanel.on('modeChange', (mode) => {
                console.log('[App] 控制模式切换:', mode);
            });
            this.controlPanel.on('manualAction', (params) => {
                console.log('[App] 手动动作下发:', params);
                this.showToast(`手动参数已下发: ${params.frequency}次/分, ${params.stroke}cm`, 'info');
            });
            this.controlPanel.on('quickAction', (action) => {
                this.showToast(`快捷动作 "${action.type}" 已执行`, 'info');
            });
            this.controlPanel.on('rlReset', (id) => {
                this.showToast(`炉 ${id} 的控制器已重置`, 'info');
            });
            console.log('[App] ControlPanel 初始化完成');
        } catch (e) {
            console.error('[App] ControlPanel 初始化失败:', e);
        }
    }

    async loadFurnaceConfigs() {
        const result = await this.api.listFurnaces();
        if (result.success && result.data) {
            result.data.forEach(config => {
                this.furnaceConfigs[config.furnace_id] = config;
            });
            this.updateFurnaceSelector();
            console.log('[App] 加载炉配置:', Object.keys(this.furnaceConfigs));
        } else {
            this.furnaceConfigs = {
                'HAN-001': {
                    furnace_id: 'HAN-001',
                    furnace_name: '汉代炒钢炉一号',
                    furnace_type: 'Han_Chaogang',
                    volume_m3: 2.5,
                    max_temperature: 1450.0,
                    target_temp_min: 1200.0,
                    target_temp_max: 1350.0,
                },
                'MING-001': {
                    furnace_id: 'MING-001',
                    furnace_name: '明代高炉一号',
                    furnace_type: 'Ming_Blast',
                    volume_m3: 8.0,
                    max_temperature: 1600.0,
                    target_temp_min: 1350.0,
                    target_temp_max: 1500.0,
                }
            };
        }

        const config = this.furnaceConfigs[this.currentFurnace];
        if (config && this.tempChart) {
            this.tempChart.setTargetRange(config.target_temp_min, config.target_temp_max);
        }
    }

    updateFurnaceSelector() {
        const selector = document.getElementById('furnaceSelector');
        if (!selector) return;

        selector.innerHTML = '';
        Object.values(this.furnaceConfigs).forEach(config => {
            const option = document.createElement('option');
            option.value = config.furnace_id;
            option.textContent = `${config.furnace_name} (${config.furnace_id})`;
            if (config.furnace_id === this.currentFurnace) {
                option.selected = true;
            }
            selector.appendChild(option);
        });
    }

    switchFurnace(furnaceId) {
        this.currentFurnace = furnaceId;
        this.alarms = [];
        this.updateAlarmDisplay();

        const config = this.furnaceConfigs[furnaceId];
        if (config) {
            const type = config.furnace_type === 'Ming_Blast' ? 'MING' : 'HAN';
            if (this.furnace3d) {
                this.furnace3d.setFurnaceType(type);
            }
            if (this.tempChart) {
                this.tempChart.setTargetRange(config.target_temp_min, config.target_temp_max);
            }
        }

        this.loadInitialData();
        this.showToast('info', '切换冶炼炉', `已切换至 ${config?.furnace_name || furnaceId}`);
    }

    initWebSocket() {
        const wsUrl = CONFIG.API_BASE_URL.replace(/^http/, 'ws') + '/ws';
        this.ws = new WSClient(wsUrl, { autoReconnect: true });

        this.ws.on('connecting', () => {
            this.updateConnectionStatus('connecting');
        });

        this.ws.on('open', () => {
            this.updateConnectionStatus('connected');
            console.log('[App] WebSocket 连接成功');
            this.showToast('success', '连接成功', '实时数据通道已建立');
        });

        this.ws.on('close', () => {
            this.updateConnectionStatus('disconnected');
        });

        this.ws.on('error', (error) => {
            console.error('[App] WebSocket 错误:', error);
        });

        this.ws.on('sensor_data', (msg) => {
            if (msg.data && (!msg.furnace_id || msg.furnace_id === this.currentFurnace)) {
                this.processSensorReading(msg.data);
            }
        });

        this.ws.on('message', (msg) => {
            if (msg.msg_type === 'alarm' && (!msg.furnace_id || msg.furnace_id === this.currentFurnace)) {
                this.processAlarm(msg.data);
            } else if (msg.msg_type === 'control_action' && msg.furnace_id === this.currentFurnace) {
                this.processControlAction(msg.data);
            }
        });

        this.ws.connect();
    }

    async loadInitialData() {
        const result = await this.api.getLatestReading(this.currentFurnace);
        if (result.success && result.data) {
            this.processSensorReading(result.data);
        }

        const alarmResult = await this.api.listAlarms(24);
        if (alarmResult.success && alarmResult.data) {
            const relevant = alarmResult.data.filter(a => a.furnace_id === this.currentFurnace);
            relevant.forEach(alarm => this.alarms.unshift(alarm));
            this.alarms = this.alarms.slice(0, 50);
            this.updateAlarmDisplay();
        }

        const rlResult = await this.api.getRLStatusFor(this.currentFurnace);
        if (rlResult.success && rlResult.data) {
            this.rlStatus = rlResult.data;
            this.updateRLDisplay();
        }

        const fieldResult = await this.api.getTempField(this.currentFurnace, 96);
        if (fieldResult.success && fieldResult.data) {
            this.tempField.updateData(fieldResult.data);
        }

        this.updateRLStatus();
    }

    processSensorReading(reading) {
        if (!reading) return;
        if (reading.furnace_id && reading.furnace_id !== this.currentFurnace) return;

        this.lastReading = reading;
        this.updateMetricsDisplay(reading);
        this.updateTempZones(reading);

        if (this.furnace3d) {
            const zones = reading.temp_zones || [
                reading.temp_zone_top,
                reading.temp_zone_upper,
                reading.temp_zone_middle,
                reading.temp_zone_lower,
                reading.temp_zone_hearth
            ];
            this.furnace3d.updateTemp(reading.furnace_temp, zones);
        }

        if (this.bellows) {
            this.bellows.updateData(
                reading.push_pull_frequency,
                reading.stroke_length,
                reading.wind_pressure,
                reading.air_volume
            );
            this.bellows.setTempData(reading.furnace_temp);
            const statusEl = document.getElementById('bellowsStatus');
            this.bellows.updateStatus(statusEl);
        }

        if (this.tempChart) {
            this.tempChart.addTempPoint(reading.furnace_temp);
        }

        if (this.tempField) {
            const zones = [
                reading.temp_zone_top,
                reading.temp_zone_upper,
                reading.temp_zone_middle,
                reading.temp_zone_lower,
                reading.temp_zone_hearth
            ];
            this.tempField.updateData({
                zones,
                temp_min: (reading.furnace_temp - 300),
                temp_max: (reading.temp_zone_hearth || reading.furnace_temp) + 100
            });
        }

        this.updateThermoDisplay(reading);
    }

    processAlarm(alarm) {
        if (!alarm) return;
        this.alarms.unshift(alarm);
        if (this.alarms.length > 50) this.alarms.pop();
        this.updateAlarmDisplay();

        const level = (alarm.alarm_level || '').toLowerCase();
        const typeNames = {
            'TEMP_TOO_HIGH': '炉温过高',
            'TEMP_TOO_LOW': '炉温过低',
            'CO_ACCUMULATION': 'CO积聚',
            'PRESSURE_ABNORMAL': '风压异常',
            'EFFICIENCY_LOW': '效率过低',
            'SYSTEM_ERROR': '系统错误'
        };
        const title = typeNames[alarm.alarm_type] || alarm.alarm_type || '告警';
        const icon = level === 'fatal' ? '🚨' : level === 'critical' ? '⚠️' : '❗';

        this.showToast(level, `${icon} ${title}`, alarm.message || '');
    }

    processControlAction(action) {
        if (!action) return;
        this.recommendedAction = action;

        const sliderFreq = document.getElementById('sliderFreq');
        const sliderStroke = document.getElementById('sliderStroke');
        const freqVal = document.getElementById('sliderFreqVal');
        const strokeVal = document.getElementById('sliderStrokeVal');

        if (this.autoControlEnabled) {
            if (sliderFreq) sliderFreq.value = Math.round(action.frequency);
            if (sliderStroke) sliderStroke.value = Math.round(action.stroke);
            if (freqVal) freqVal.textContent = Math.round(action.frequency);
            if (strokeVal) strokeVal.textContent = Math.round(action.stroke);
        }
    }

    updateConnectionStatus(status) {
        const dot = document.getElementById('statusDot');
        const text = document.getElementById('statusText');

        if (dot && text) {
            dot.className = 'status-dot ' + status;
            const labels = {
                'connecting': '连接中...',
                'connected': '已连接',
                'disconnected': '已断开'
            };
            text.textContent = labels[status] || status;
        }
    }

    updateMetricsDisplay(r) {
        this.setMetric('valFurnaceTemp', 'barTemp', r.furnace_temp, '°C',
            r.furnace_temp / 1600 * 100, true);
        this.setMetric('valCo', 'barCo', r.co_concentration, '%',
            (r.co_concentration / 8) * 100, true);
        this.setMetric('valFreq', 'barFreq', r.push_pull_frequency, '',
            ((r.push_pull_frequency - 10) / 50) * 100, true);
        this.setMetric('valStroke', 'barStroke', r.stroke_length, '',
            ((r.stroke_length - 15) / 65) * 100, true);
        this.setTextValue('valPressure', r.wind_pressure.toFixed(0), ' Pa');
        this.setTextValue('valAirVol', r.air_volume.toFixed(4), ' m³/s');
        this.setMetric('valEff', 'barEff', r.energy_efficiency, '%',
            r.energy_efficiency, true);
        this.setTextValue('valIron', r.pig_iron_output.toFixed(1), ' kg');

        const tempEl = document.getElementById('metricTemp');
        const config = this.furnaceConfigs[this.currentFurnace];
        if (tempEl && config) {
            const isHigh = r.furnace_temp > config.target_temp_max + 50;
            const isLow = r.furnace_temp < config.target_temp_min - 50;
            tempEl.style.borderColor = isHigh ? 'var(--accent-red)'
                : isLow ? 'var(--accent-blue)'
                    : 'var(--border-color)';
        }
    }

    setMetric(valElId, barElId, value, unit, percent, fix1) {
        const valEl = document.getElementById(valElId);
        const barEl = document.getElementById(barElId);
        if (valEl) {
            valEl.textContent = fix1 ? value.toFixed(1) : Math.round(value);
        }
        if (barEl) {
            const p = Math.max(0, Math.min(100, percent));
            barEl.style.width = p + '%';
        }
    }

    setTextValue(elId, value, suffix) {
        const el = document.getElementById(elId);
        if (el) el.textContent = value + suffix;
    }

    updateTempZones(r) {
        const zones = [
            { id: 'Top', val: r.temp_zone_top },
            { id: 'Upper', val: r.temp_zone_upper },
            { id: 'Middle', val: r.temp_zone_middle },
            { id: 'Lower', val: r.temp_zone_lower },
            { id: 'Hearth', val: r.temp_zone_hearth }
        ];

        const min = Math.min(...zones.map(z => z.val)) - 50;
        const max = Math.max(...zones.map(z => z.val)) + 50;
        const range = max - min || 1;

        zones.forEach(z => {
            const bar = document.getElementById('zone' + z.id);
            const val = document.getElementById('valZone' + z.id);
            const p = ((z.val - min) / range) * 100;

            if (bar) {
                const tempRatio = ((z.val - 400) / 1200).clamp ? ((z.val - 400) / 1200) : Math.max(0, Math.min(1, (z.val - 400) / 1200));
                const hue = 200 - tempRatio * 170;
                bar.style.background = `linear-gradient(90deg,
                    hsl(${hue}, 80%, 40%), hsl(${Math.max(0, hue - 40)}, 90%, 55%))`;
            }
            if (val) val.textContent = `${Math.round(z.val)}°C`;
        });
    }

    updateRLDisplay() {
        const s = this.rlStatus || {};
        this.setTextValue('rlEpisode', s.episode || 0, '');
        this.setTextValue('rlStep', s.step || 0, '');
        if (document.getElementById('rlEpsilon')) {
            document.getElementById('rlEpsilon').textContent = (s.epsilon || 0).toFixed(3);
        }
        if (document.getElementById('rlReward')) {
            document.getElementById('rlReward').textContent = (s.last_reward || 0).toFixed(1);
        }
        this.setTextValue('rlBuffer', s.buffer_size || 0, '');
    }

    async updateRLStatus() {
        const result = await this.api.getRLStatusFor(this.currentFurnace);
        if (result.success && result.data) {
            this.rlStatus = result.data;
            this.updateRLDisplay();
        }
    }

    updateThermoDisplay(r) {
        if (document.getElementById('thermoRate')) {
            document.getElementById('thermoRate').textContent =
                `${(r.reaction_rate || 0).toFixed(4)} mol/s`;
        }
    }

    updateAlarmDisplay() {
        const listEl = document.getElementById('alarmList');
        const badgeEl = document.getElementById('alarmCount');
        if (badgeEl) {
            badgeEl.textContent = this.alarms.filter(a => !a.acknowledged).length;
        }

        if (!listEl) return;

        if (this.alarms.length === 0) {
            listEl.innerHTML = '<div class="alarm-empty">暂无告警</div>';
            return;
        }

        listEl.innerHTML = this.alarms.slice(0, 10).map(alarm => {
            const level = (alarm.alarm_level || 'warning').toLowerCase();
            const type = alarm.alarm_type || 'UNKNOWN';
            const time = alarm.timestamp
                ? new Date(alarm.timestamp).toLocaleTimeString('zh-CN')
                : '--:--:--';
            const ackClass = alarm.acknowledged ? ' opacity-50' : '';
            const currentVal = alarm.current_value ? alarm.current_value.toFixed(1) : 'N/A';
            const thresholdVal = alarm.threshold_value ? alarm.threshold_value.toFixed(1) : 'N/A';

            return `
                <div class="alarm-item ${level}${ackClass}">
                    <div class="alarm-header">
                        <span class="alarm-type">${type}</span>
                        <span class="alarm-time">${time}</span>
                    </div>
                    <div class="alarm-message">${alarm.message || ''}</div>
                    <div class="alarm-values">
                        当前: ${currentVal} / 阈值: ${thresholdVal}
                    </div>
                </div>
            `;
        }).join('');
    }

    async applyManualControl() {
        const freq = parseInt(document.getElementById('sliderFreq')?.value || 30);
        const stroke = parseInt(document.getElementById('sliderStroke')?.value || 45);

        const result = await this.api.setManualAction(this.currentFurnace, {
            frequency: freq,
            stroke: stroke
        });

        if (result.success) {
            this.showToast('success', '控制参数已应用',
                `频率: ${freq} 次/分, 行程: ${stroke} cm`);
            this.autoControlEnabled = false;
            this.updateAutoControlButton();

            if (this.bellows) {
                this.bellows.updateData(freq, stroke,
                    this.lastReading?.wind_pressure || 1000,
                    this.lastReading?.air_volume || 0.5);
            }
        } else {
            this.showToast('critical', '应用失败', result.message || '未知错误');
        }
    }

    toggleAutoControl() {
        this.autoControlEnabled = !this.autoControlEnabled;
        this.updateAutoControlButton();

        if (this.autoControlEnabled) {
            this.showToast('success', '自动RL控制已启用',
                '强化学习算法将自动优化鼓风参数');
        } else {
            this.showToast('info', '自动控制已禁用',
                '请手动设置鼓风参数');
        }
    }

    updateAutoControlButton() {
        const btn = document.getElementById('btnAutoControl');
        if (!btn) return;
        if (this.autoControlEnabled) {
            btn.textContent = '禁用自动RL';
            btn.style.background = 'linear-gradient(135deg, var(--accent-green), #22c55e)';
            btn.style.color = 'var(--bg-primary)';
        } else {
            btn.textContent = '启用自动RL';
            btn.style.background = '';
            btn.style.color = '';
        }
    }

    updateCurrentTime() {
        const el = document.getElementById('currentTime');
        if (el) {
            const now = new Date();
            el.textContent = now.toLocaleString('zh-CN', {
                year: 'numeric',
                month: '2-digit',
                day: '2-digit',
                hour: '2-digit',
                minute: '2-digit',
                second: '2-digit',
                hour12: false
            });
        }
    }

    showToast(level, title, message) {
        const container = document.getElementById('toastContainer');
        if (!container) return;

        const icons = {
            'success': '✅',
            'info': 'ℹ️',
            'warning': '⚠️',
            'critical': '🚨',
            'fatal': '💀'
        };

        const toast = document.createElement('div');
        toast.className = `toast ${level}`;
        toast.innerHTML = `
            <span class="toast-icon">${icons[level] || '📢'}</span>
            <div class="toast-content">
                <div class="toast-title">${title}</div>
                ${message ? `<div class="toast-message">${message}</div>` : ''}
            </div>
        `;

        container.appendChild(toast);

        setTimeout(() => {
            if (toast.parentNode) {
                toast.style.animation = 'toastOut 0.3s ease forwards';
                setTimeout(() => toast.remove(), 300);
            }
        }, 5000);
    }
}

if (!Math.clamp) {
    Math.clamp = function(value, min, max) {
        return Math.min(Math.max(value, min), max);
    };
}

document.addEventListener('DOMContentLoaded', () => {
    window.metallurgyApp = new MetallurgyApp();
    console.log('[App] 古代风箱鼓风冶铁仿真系统已启动');
});

export default MetallurgyApp;
