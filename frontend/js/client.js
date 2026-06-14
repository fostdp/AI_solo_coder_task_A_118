class WSClient {
    constructor(url, options = {}) {
        this.url = url;
        this.options = options;
        this.ws = null;
        this.reconnectAttempts = 0;
        this.maxReconnectAttempts = options.maxReconnectAttempts || 20;
        this.reconnectInterval = options.reconnectInterval || 3000;
        this.listeners = {};
        this.autoReconnect = options.autoReconnect !== false;
        this.isManualClose = false;
    }

    connect() {
        this.isManualClose = false;
        this._connect();
    }

    _connect() {
        try {
            this._emit('connecting');
            this.ws = new WebSocket(this.url);

            this.ws.onopen = () => {
                this.reconnectAttempts = 0;
                this._emit('open');
            };

            this.ws.onmessage = (event) => {
                try {
                    const data = JSON.parse(event.data);
                    this._emit('message', data);
                    if (data.msg_type) {
                        this._emit(data.msg_type, data);
                    }
                } catch (e) {
                    this._emit('error', e);
                }
            };

            this.ws.onerror = (error) => {
                this._emit('error', error);
            };

            this.ws.onclose = (event) => {
                this._emit('close', event);
                if (this.autoReconnect && !this.isManualClose) {
                    this._scheduleReconnect();
                }
            };
        } catch (e) {
            this._emit('error', e);
            if (this.autoReconnect) {
                this._scheduleReconnect();
            }
        }
    }

    _scheduleReconnect() {
        if (this.reconnectAttempts >= this.maxReconnectAttempts) {
            this._emit('max_reconnect_exceeded');
            return;
        }

        const delay = Math.min(
            this.reconnectInterval * Math.pow(1.5, this.reconnectAttempts),
            30000
        );

        this.reconnectAttempts++;
        this._emit('reconnect_scheduled', {
            attempt: this.reconnectAttempts,
            delay
        });

        setTimeout(() => {
            this._connect();
        }, delay);
    }

    send(data) {
        if (this.ws && this.ws.readyState === WebSocket.OPEN) {
            const payload = typeof data === 'string' ? data : JSON.stringify(data);
            this.ws.send(payload);
            return true;
        }
        return false;
    }

    on(event, callback) {
        if (!this.listeners[event]) {
            this.listeners[event] = [];
        }
        this.listeners[event].push(callback);
        return () => this.off(event, callback);
    }

    off(event, callback) {
        if (!this.listeners[event]) return;
        if (!callback) {
            this.listeners[event] = [];
            return;
        }
        this.listeners[event] = this.listeners[event].filter(cb => cb !== callback);
    }

    _emit(event, ...args) {
        if (this.listeners[event]) {
            this.listeners[event].forEach(cb => {
                try {
                    cb(...args);
                } catch (e) {
                    console.error(`[WS] Listener error for ${event}:`, e);
                }
            });
        }
    }

    close(code = 1000, reason = '') {
        this.isManualClose = true;
        if (this.ws) {
            this.ws.close(code, reason);
        }
    }

    get state() {
        if (!this.ws) return 'CLOSED';
        const states = ['CONNECTING', 'OPEN', 'CLOSING', 'CLOSED'];
        return states[this.ws.readyState] || 'UNKNOWN';
    }

    get isConnected() {
        return this.ws && this.ws.readyState === WebSocket.OPEN;
    }
}

class ApiClient {
    constructor(baseUrl) {
        this.baseUrl = baseUrl.replace(/\/$/, '');
    }

    async _request(method, path, data = null, options = {}) {
        const url = `${this.baseUrl}${path}`;
        const config = {
            method,
            headers: {
                'Content-Type': 'application/json',
                ...options.headers
            },
            ...options
        };

        if (data && (method === 'POST' || method === 'PUT' || method === 'PATCH')) {
            config.body = JSON.stringify(data);
        }

        try {
            const response = await fetch(url, config);
            const text = await response.text();
            let result;
            try {
                result = text ? JSON.parse(text) : {};
            } catch {
                result = { success: response.ok, message: text };
            }
            return result;
        } catch (error) {
            console.error(`[API] ${method} ${path} failed:`, error);
            return { success: false, message: error.message };
        }
    }

    get(path, options = {}) {
        return this._request('GET', path, null, options);
    }

    post(path, data, options = {}) {
        return this._request('POST', path, data, options);
    }

    put(path, data, options = {}) {
        return this._request('PUT', path, data, options);
    }

    delete(path, options = {}) {
        return this._request('DELETE', path, null, options);
    }

    async getHealth() {
        return this.get('/api/health');
    }

    async getStatus() {
        return this.get('/api/status');
    }

    async listFurnaces() {
        return this.get('/api/furnaces/');
    }

    async getFurnace(furnaceId) {
        return this.get(`/api/furnaces/${furnaceId}`);
    }

    async getLatestReading(furnaceId) {
        return this.get(`/api/furnaces/${furnaceId}/reading/latest`);
    }

    async getReadingHistory(furnaceId, params = {}) {
        const query = new URLSearchParams(params).toString();
        return this.get(`/api/furnaces/${furnaceId}/reading/history${query ? '?' + query : ''}`);
    }

    async getTempField(furnaceId, resolution = 64) {
        return this.get(`/api/furnaces/${furnaceId}/temp_field?resolution=${resolution}`);
    }

    async getProduction(furnaceId, days = 30) {
        return this.get(`/api/furnaces/${furnaceId}/production?hours=${days * 24}`);
    }

    async reportSensor(data) {
        return this.post('/api/sensor/report', data);
    }

    async predictThermo(furnaceId, action) {
        return this.post(`/api/thermo/predict/${furnaceId}`, action);
    }

    async getThermoParams(furnaceId) {
        return this.get(`/api/thermo/params/${furnaceId}`);
    }

    async setThermoParams(furnaceId, params) {
        return this.put(`/api/thermo/params/${furnaceId}`, params);
    }

    async listAlarms(hours = 24) {
        return this.get(`/api/alarms/?hours=${hours}`);
    }

    async acknowledgeAlarm(eventId) {
        return this.put(`/api/alarms/${eventId}/ack`);
    }

    async getRLStatus() {
        return this.get('/api/rl/status');
    }

    async getRLStatusFor(furnaceId) {
        return this.get(`/api/rl/status/${furnaceId}`);
    }

    async setManualAction(furnaceId, action) {
        return this.post(`/api/rl/action/${furnaceId}`, action);
    }
}

export { WSClient, ApiClient };
