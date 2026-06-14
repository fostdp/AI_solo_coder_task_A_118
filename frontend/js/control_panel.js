import { ApiClient } from './client.js';

export class ControlPanel {
  constructor(options = {}) {
    this.api = new ApiClient(options.apiBaseUrl || 'http://127.0.0.1:8080');
    this.containerId = options.containerId || 'controlPanel';
    this.currentFurnaceId = options.initialFurnaceId || 'HAN-001';
    this.furnaces = options.furnaces || [];
    this.controlMode = options.initialMode || 'auto';
    this.currentAlgo = options.initialAlgo || 'q_learning';
    this.manualFrequency = options.manualFrequency || 25;
    this.manualStroke = options.manualStroke || 35;

    this.eventListeners = {};
    this._boundElements = {};

    this._init();
  }

  on(event, callback) {
    if (!this.eventListeners[event]) {
      this.eventListeners[event] = [];
    }
    this.eventListeners[event].push(callback);
  }

  _emit(event, data) {
    const listeners = this.eventListeners[event];
    if (!listeners) return;
    for (const cb of listeners) {
      try {
        cb(data);
      } catch (e) {
        console.error('[ControlPanel] event listener error:', event, e);
      }
    }
  }

  _init() {
    this.container = document.getElementById(this.containerId);
    if (!this.container) {
      throw new Error(`Control panel container #${this.containerId} not found`);
    }
    this._buildFurnaceSelector();
    this._buildControlMode();
    this._buildAlgoSwitch();
    this._buildManualSliders();
    this._buildRLStatus();
    this._buildParamIdStatus();
    this._buildActionButtons();
    this._bindEvents();
    this._updateUIState();
  }

  _buildFurnaceSelector() {
    const html = `
      <div class="panel-section" id="cp-furnace-section">
        <h3 class="panel-title">冶炼炉选择</h3>
        <div class="furnace-selector">
          <label for="cp-furnace-select" class="cp-label">当前冶炼炉:</label>
          <select id="cp-furnace-select" class="cp-select">
            ${this.furnaces.length ? this.furnaces.map(f => `
              <option value="${f.furnace_id}" ${f.furnace_id === this.currentFurnaceId ? 'selected' : ''}>
                ${f.furnace_name} (${f.furnace_id})
              </option>
            `).join('') : `
              <option value="HAN-001">汉代炒钢炉一号 (HAN-001)</option>
              <option value="MING-001">明代高炉一号 (MING-001)</option>
            `}
          </select>
          <div id="cp-furnace-info" class="cp-subtext">
            目标温度: <span id="cp-temp-range">1200~1350°C</span>
          </div>
        </div>
      </div>
    `;
    this._append(html);
  }

  _buildControlMode() {
    const html = `
      <div class="panel-section" id="cp-mode-section">
        <h3 class="panel-title">控制模式</h3>
        <div class="mode-tabs">
          <button id="cp-mode-auto" class="cp-tab active" data-mode="auto">
            <span class="dot auto"></span>
            自动RL控制
          </button>
          <button id="cp-mode-manual" class="cp-tab" data-mode="manual">
            <span class="dot manual"></span>
            手动干预
          </button>
          <button id="cp-mode-pause" class="cp-tab" data-mode="pause">
            <span class="dot pause"></span>
            暂停
          </button>
        </div>
        <div id="cp-mode-description" class="cp-subtext">
          强化学习自动优化风箱参数
        </div>
      </div>
    `;
    this._append(html);
  }

  _buildAlgoSwitch() {
    const html = `
      <div class="panel-section" id="cp-algo-section">
        <h3 class="panel-title">控制算法</h3>
        <div class="algo-switch">
          <label class="cp-radio">
            <input type="radio" name="cp-algo" value="q_learning" ${this.currentAlgo === 'q_learning' ? 'checked' : ''}>
            <span>Q-Learning <em class="tag">快速</em></span>
          </label>
          <label class="cp-radio">
            <input type="radio" name="cp-algo" value="ddpg" ${this.currentAlgo === 'ddpg' ? 'checked' : ''}>
            <span>DDPG <em class="tag">精确</em></span>
          </label>
        </div>
        <button id="cp-algo-apply" class="cp-btn small">切换算法</button>
        <div id="cp-algo-status" class="cp-subtext algo-status">
          当前: Q-Learning
        </div>
      </div>
    `;
    this._append(html);
  }

  _buildManualSliders() {
    const html = `
      <div class="panel-section" id="cp-manual-section">
        <h3 class="panel-title">手动参数设置
          <span id="cp-manual-badge" class="badge disabled">禁用</span>
        </h3>
        <div class="slider-row">
          <label class="cp-label" for="cp-freq-slider">
            推拉频率: <span id="cp-freq-value">${this.manualFrequency}</span> 次/分
          </label>
          <input type="range" id="cp-freq-slider" class="cp-slider"
                 min="10" max="60" step="1" value="${this.manualFrequency}" disabled>
        </div>
        <div class="slider-range">
          <span>10</span><span class="mid">35</span><span>60</span>
        </div>
        <div class="slider-row">
          <label class="cp-label" for="cp-stroke-slider">
            行程长度: <span id="cp-stroke-value">${this.manualStroke}</span> cm
          </label>
          <input type="range" id="cp-stroke-slider" class="cp-slider"
                 min="15" max="80" step="1" value="${this.manualStroke}" disabled>
        </div>
        <div class="slider-range">
          <span>15</span><span class="mid">47</span><span>80</span>
        </div>
        <button id="cp-manual-apply" class="cp-btn warning" disabled>下发手动参数</button>
        <button id="cp-manual-release" class="cp-btn ghost small" disabled>解除手动覆盖</button>
      </div>
    `;
    this._append(html);
  }

  _buildRLStatus() {
    const html = `
      <div class="panel-section" id="cp-rl-section">
        <h3 class="panel-title">强化学习状态</h3>
        <div class="rl-grid">
          <div class="rl-metric">
            <span class="rl-label">探索率 ε</span>
            <div class="rl-bar-wrap">
              <div id="cp-epsilon-bar" class="rl-bar epsilon" style="width:80%"></div>
            </div>
            <span id="cp-epsilon-value" class="rl-value">0.80</span>
          </div>
          <div class="rl-metric">
            <span class="rl-label">平均奖励</span>
            <span id="cp-avg-reward" class="rl-value big">--</span>
          </div>
          <div class="rl-metric">
            <span class="rl-label">训练步数</span>
            <span id="cp-episodes" class="rl-value">0</span>
          </div>
          <div class="rl-metric">
            <span class="rl-label">Q表规模</span>
            <span id="cp-qsize" class="rl-value">0</span>
          </div>
        </div>
        <button id="cp-rl-reset" class="cp-btn ghost small">重置此炉控制器</button>
      </div>
    `;
    this._append(html);
  }

  _buildParamIdStatus() {
    const html = `
      <div class="panel-section" id="cp-pid-section">
        <h3 class="panel-title">在线参数辨识</h3>
        <div class="pid-row">
          <span class="pid-label">辨识置信度</span>
          <div class="rl-bar-wrap">
            <div id="cp-pid-confidence" class="rl-bar confidence" style="width:0%"></div>
          </div>
          <span id="cp-pid-conf-value" class="rl-value">0.00</span>
        </div>
        <div class="param-grid">
          <div><span class="pid-label">活化能 Ea</span><span id="cp-pid-ea" class="pid-value">--</span></div>
          <div><span class="pid-label">指前因子 A</span><span id="cp-pid-a" class="pid-value">--</span></div>
          <div><span class="pid-label">热损失系数</span><span id="cp-pid-hlc" class="pid-value">--</span></div>
          <div><span class="pid-label">辨识样本</span><span id="cp-pid-samples" class="pid-value">0</span></div>
        </div>
        <div id="cp-pid-status" class="cp-subtext">
          等待采集足够样本...
        </div>
      </div>
    `;
    this._append(html);
  }

  _buildActionButtons() {
    const html = `
      <div class="panel-section" id="cp-actions-section">
        <h3 class="panel-title">快捷操作</h3>
        <div class="action-grid">
          <button id="cp-action-heatup" class="cp-btn warning" title="快速升温 (最大风箱)">
            🔥 快速升温
          </button>
          <button id="cp-action-cooldown" class="cp-btn info" title="紧急减风降温">
            ❄️ 紧急降温
          </button>
          <button id="cp-action-hold" class="cp-btn" title="维持当前参数">
            ⏸ 保持参数
          </button>
          <button id="cp-action-refresh" class="cp-btn ghost" title="刷新当前状态">
            🔄 刷新状态
          </button>
        </div>
      </div>
    `;
    this._append(html);
  }

  _append(html) {
    const tmp = document.createElement('template');
    tmp.innerHTML = html.trim();
    while (tmp.content.firstChild) {
      this.container.appendChild(tmp.content.firstChild);
    }
  }

  _bindEvents() {
    this._onChange('cp-furnace-select', (e) => {
      this.currentFurnaceId = e.target.value;
      this._updateFurnaceInfo();
      this._emit('furnaceChange', this.currentFurnaceId);
      this.refreshStatus();
    });

    document.querySelectorAll('#cp-mode-section .cp-tab').forEach(btn => {
      btn.addEventListener('click', () => {
        this.controlMode = btn.dataset.mode;
        this._updateUIState();
        this._emit('modeChange', this.controlMode);
      });
    });

    this._onClick('cp-algo-apply', async () => {
      const selected = document.querySelector('input[name="cp-algo"]:checked');
      if (!selected) return;
      const algo = selected.value;
      try {
        const resp = await this.api.setControlAlgorithm(algo);
        if (resp && resp.algo) {
          this.currentAlgo = resp.algo.algo || algo;
          this._updateAlgoStatus();
          this._emit('algoChange', this.currentAlgo);
        }
      } catch (e) {
        console.error('切换算法失败:', e);
      }
    });

    this._onInput('cp-freq-slider', (e) => {
      this.manualFrequency = parseInt(e.target.value, 10);
      this._set('cp-freq-value', this.manualFrequency);
    });

    this._onInput('cp-stroke-slider', (e) => {
      this.manualStroke = parseInt(e.target.value, 10);
      this._set('cp-stroke-value', this.manualStroke);
    });

    this._onClick('cp-manual-apply', async () => {
      try {
        const res = await this.api.setManualAction(this.currentFurnaceId, {
          frequency: this.manualFrequency,
          stroke: this.manualStroke,
          duration_secs: 300,
        });
        if (res) {
          this._emit('manualAction', {
            frequency: this.manualFrequency,
            stroke: this.manualStroke,
          });
          this._flashButton('cp-manual-apply', 'success');
        }
      } catch (e) {
        console.error('下发手动参数失败:', e);
        this._flashButton('cp-manual-apply', 'error');
      }
    });

    this._onClick('cp-manual-release', async () => {
      try {
        await this.api.clearManualOverride(this.currentFurnaceId);
        this._emit('manualReleased', this.currentFurnaceId);
        this._flashButton('cp-manual-release', 'success');
      } catch (e) {
        console.error('解除手动覆盖失败:', e);
      }
    });

    this._onClick('cp-rl-reset', async () => {
      if (!confirm(`确定重置炉 ${this.currentFurnaceId} 的强化学习？`)) return;
      try {
        await this.api.resetQL(this.currentFurnaceId);
        this._emit('rlReset', this.currentFurnaceId);
        this.refreshStatus();
      } catch (e) {
        console.error('重置RL失败:', e);
      }
    });

    const quickActions = {
      'cp-action-heatup': { frequency: 60, stroke: 80, type: 'heatup' },
      'cp-action-cooldown': { frequency: 12, stroke: 20, type: 'cooldown' },
    };

    for (const [id, params] of Object.entries(quickActions)) {
      this._onClick(id, async () => {
        try {
          const res = await this.api.setManualAction(this.currentFurnaceId, {
            frequency: params.frequency,
            stroke: params.stroke,
            duration_secs: 120,
            reason: params.type,
          });
          this._emit('quickAction', { ...params, furnace: this.currentFurnaceId });
          this._flashButton(id, 'success');
        } catch (e) {
          this._flashButton(id, 'error');
        }
      });
    }

    this._onClick('cp-action-hold', () => {
      this._emit('holdAction', { furnace: this.currentFurnaceId });
      this._flashButton('cp-action-hold', 'success');
    });

    this._onClick('cp-action-refresh', () => {
      this.refreshStatus();
    });
  }

  async refreshStatus() {
    try {
      const status = await Promise.all([
        this.api.getQLStatus(this.currentFurnaceId).catch(() => null),
        this.api.getParamIdStatus(this.currentFurnaceId).catch(() => null),
        this.api.getControlAlgorithm().catch(() => null),
      ]);

      const [ql, pid, algo] = status;

      if (ql && ql.data) {
        const s = ql.data;
        this._updateRL(s.epsilon || 0, s.avg_reward || 0, s.episodes || 0, s.q_table_size || 0);
      }

      if (pid && pid.data) {
        this._updateParamId(
          pid.data.confidence || 0,
          pid.data.activation_energy,
          pid.data.pre_exponential_factor,
          pid.data.heat_loss_coefficient,
          pid.data.sample_count || 0
        );
      }

      if (algo && algo.data && algo.data.current) {
        this.currentAlgo = algo.data.current;
        document.querySelectorAll('input[name="cp-algo"]').forEach(r => {
          r.checked = r.value === this.currentAlgo;
        });
        this._updateAlgoStatus();
      }
    } catch (e) {
      console.error('刷新状态失败:', e);
    }
  }

  _updateRL(epsilon, avgReward, episodes, qSize) {
    const bar = document.getElementById('cp-epsilon-bar');
    if (bar) bar.style.width = `${(epsilon * 100).toFixed(1)}%`;
    this._set('cp-epsilon-value', epsilon.toFixed(3));
    this._set('cp-avg-reward', typeof avgReward === 'number' ? avgReward.toFixed(1) : '--');
    this._set('cp-episodes', episodes);
    this._set('cp-qsize', qSize);
  }

  _updateParamId(conf, ea, a, hlc, samples) {
    const bar = document.getElementById('cp-pid-confidence');
    if (bar) bar.style.width = `${(conf * 100).toFixed(0)}%`;
    this._set('cp-pid-conf-value', conf.toFixed(2));
    this._set('cp-pid-ea', ea ? `${(ea / 1000).toFixed(1)} kJ/mol` : '--');
    this._set('cp-pid-a', a ? a.toExponential(2) : '--');
    this._set('cp-pid-hlc', hlc ? hlc.toFixed(4) : '--');
    this._set('cp-pid-samples', samples);

    const status = document.getElementById('cp-pid-status');
    if (status) {
      if (conf > 0.35) {
        status.innerHTML = '<span class="ok">✅ 参数已稳定，用于热力学计算</span>';
      } else if (samples >= 5) {
        status.innerHTML = '<span class="warn">⚠️ 样本不足，辨识中...</span>';
      } else {
        status.textContent = '等待采集足够样本...';
      }
    }
  }

  _updateAlgoStatus() {
    const el = document.getElementById('cp-algo-status');
    if (!el) return;
    const map = {
      'q_learning': 'Q-Learning (离散化，收敛快)',
      'ddpg': 'DDPG (连续动作，精度高)',
      'QLearning': 'Q-Learning (离散化，收敛快)',
      'Ddpg': 'DDPG (连续动作，精度高)',
    };
    el.innerHTML = `当前: <strong>${map[this.currentAlgo] || this.currentAlgo}</strong>`;
  }

  _updateUIState() {
    document.querySelectorAll('#cp-mode-section .cp-tab').forEach(btn => {
      btn.classList.toggle('active', btn.dataset.mode === this.controlMode);
    });

    const description = document.getElementById('cp-mode-description');
    const descMap = {
      auto: '强化学习自动优化风箱参数，目标温度区间稳定',
      manual: '手动设置风箱频率与行程，覆盖RL输出',
      pause: '暂停参数调整，维持当前值不变',
    };
    if (description) description.textContent = descMap[this.controlMode];

    const manualDisabled = this.controlMode !== 'manual';
    const badge = document.getElementById('cp-manual-badge');
    if (badge) {
      badge.classList.toggle('disabled', manualDisabled);
      badge.classList.toggle('active', !manualDisabled);
      badge.textContent = manualDisabled ? '禁用' : '启用';
    }

    ['cp-freq-slider', 'cp-stroke-slider'].forEach(id => {
      const el = document.getElementById(id);
      if (el) el.disabled = manualDisabled;
    });
    ['cp-manual-apply', 'cp-manual-release'].forEach(id => {
      const el = document.getElementById(id);
      if (el) el.disabled = manualDisabled;
    });
  }

  _updateFurnaceInfo() {
    const f = this.furnaces.find(x => x.furnace_id === this.currentFurnaceId);
    const el = document.getElementById('cp-temp-range');
    if (!el) return;
    if (f) {
      el.textContent = `${Math.round(f.target_temp_min)}~${Math.round(f.target_temp_max)}°C`;
    }
  }

  setFurnaceList(furnaces) {
    this.furnaces = furnaces;
    const sel = document.getElementById('cp-furnace-select');
    if (!sel) return;
    sel.innerHTML = furnaces.map(f => `
      <option value="${f.furnace_id}" ${f.furnace_id === this.currentFurnaceId ? 'selected' : ''}>
        ${f.furnace_name} (${f.furnace_id})
      </option>
    `).join('');
    this._updateFurnaceInfo();
  }

  setAction(action) {
    if (!action) return;
    if (typeof action.frequency === 'number' && this.controlMode === 'auto') {
      const fs = document.getElementById('cp-freq-slider');
      const fv = document.getElementById('cp-freq-value');
      if (fs) fs.value = Math.round(action.frequency);
      if (fv) fv.textContent = Math.round(action.frequency);
    }
    if (typeof action.stroke === 'number' && this.controlMode === 'auto') {
      const ss = document.getElementById('cp-stroke-slider');
      const sv = document.getElementById('cp-stroke-value');
      if (ss) ss.value = Math.round(action.stroke);
      if (sv) sv.textContent = Math.round(action.stroke);
    }
  }

  _set(id, text) {
    const el = document.getElementById(id);
    if (el) el.textContent = text;
  }

  _onChange(id, handler) {
    const el = document.getElementById(id);
    if (el) el.addEventListener('change', handler);
  }

  _onInput(id, handler) {
    const el = document.getElementById(id);
    if (el) el.addEventListener('input', handler);
  }

  _onClick(id, handler) {
    const el = document.getElementById(id);
    if (el) el.addEventListener('click', handler);
  }

  _flashButton(id, type) {
    const el = document.getElementById(id);
    if (!el) return;
    el.classList.add(`flash-${type}`);
    setTimeout(() => el.classList.remove(`flash-${type}`), 600);
  }
}
