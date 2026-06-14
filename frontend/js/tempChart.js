export class TemperatureChart {
    constructor(canvasId) {
        this.canvas = document.getElementById(canvasId);
        if (!this.canvas) {
            throw new Error(`Canvas ${canvasId} not found`);
        }
        this.ctx = this.canvas.getContext('2d');

        this.maxPoints = 60;
        this.tempData = [];
        this.targetMin = 1200;
        this.targetMax = 1350;
        this.displayTempMin = 600;
        this.displayTempMax = 1600;
        this.animationTime = 0;
        this.lastFrameTime = performance.now();

        this.resizeCanvas();
        this._initDemoData();

        this._animate = this._animate.bind(this);
        window.addEventListener('resize', () => this.resizeCanvas());
        this._animate();
    }

    resizeCanvas() {
        const rect = this.canvas.getBoundingClientRect();
        const dpr = window.devicePixelRatio || 1;
        this.canvas.width = rect.width * dpr;
        this.canvas.height = rect.height * dpr;
        this.ctx.scale(dpr, dpr);
        this.displayWidth = rect.width;
        this.displayHeight = rect.height;
    }

    _initDemoData() {
        const baseTemp = (this.targetMin + this.targetMax) / 2;
        for (let i = 0; i < 10; i++) {
            this.tempData.push({
                temp: baseTemp + (Math.random() - 0.5) * 100,
                time: Date.now() - (10 - i) * 10000
            });
        }
    }

    setTargetRange(min, max) {
        this.targetMin = min;
        this.targetMax = max;
        this.displayTempMin = Math.min(min - 200, 600);
        this.displayTempMax = Math.max(max + 200, 1600);
    }

    addTempPoint(temp) {
        const now = Date.now();
        if (this.tempData.length > 0 &&
            now - this.tempData[this.tempData.length - 1].time < 2000) {
            this.tempData[this.tempData.length - 1].temp = temp;
            return;
        }

        this.tempData.push({ temp, time: now });
        if (this.tempData.length > this.maxPoints) {
            this.tempData.shift();
        }
    }

    _animate() {
        const now = performance.now();
        const dt = (now - this.lastFrameTime) / 1000;
        this.lastFrameTime = now;
        this.animationTime += dt;

        this._draw();

        requestAnimationFrame(this._animate);
    }

    _draw() {
        const ctx = this.ctx;
        const W = this.displayWidth;
        const H = this.displayHeight;

        ctx.clearRect(0, 0, W, H);

        const padding = { top: 15, right: 15, bottom: 25, left: 42 };
        const chartX = padding.left;
        const chartY = padding.top;
        const chartW = W - padding.left - padding.right;
        const chartH = H - padding.top - padding.bottom;

        this._drawBackground(ctx, chartX, chartY, chartW, chartH);
        this._drawTargetZone(ctx, chartX, chartY, chartW, chartH);
        this._drawGrid(ctx, chartX, chartY, chartW, chartH);
        this._drawDataLine(ctx, chartX, chartY, chartW, chartH);
        this._drawDataPoints(ctx, chartX, chartY, chartW, chartH);
        this._drawAxis(ctx, chartX, chartY, chartW, chartH);
        this._drawCurrentValue(ctx, chartX, chartY, chartW, chartH);
    }

    _drawBackground(ctx, x, y, w, h) {
        const grad = ctx.createLinearGradient(0, y, 0, y + h);
        grad.addColorStop(0, 'rgba(30, 42, 58, 0.6)');
        grad.addColorStop(1, 'rgba(20, 30, 42, 0.8)');
        ctx.fillStyle = grad;
        this._roundRect(ctx, x, y, w, h, 4);
        ctx.fill();
    }

    _drawTargetZone(ctx, x, y, w, h) {
        const yMin = this._tempToY(this.targetMin, y, h);
        const yMax = this._tempToY(this.targetMax, y, h);
        const zoneY = Math.min(yMin, yMax);
        const zoneH = Math.abs(yMax - yMin);

        const grad = ctx.createLinearGradient(0, zoneY, 0, zoneY + zoneH);
        grad.addColorStop(0, 'rgba(74, 222, 128, 0.05)');
        grad.addColorStop(0.5, 'rgba(74, 222, 128, 0.18)');
        grad.addColorStop(1, 'rgba(74, 222, 128, 0.05)');
        ctx.fillStyle = grad;
        ctx.fillRect(x, zoneY, w, zoneH);

        ctx.strokeStyle = 'rgba(74, 222, 128, 0.5)';
        ctx.lineWidth = 1;
        ctx.setLineDash([3, 3]);

        ctx.beginPath();
        ctx.moveTo(x, yMin);
        ctx.lineTo(x + w, yMin);
        ctx.stroke();

        ctx.beginPath();
        ctx.moveTo(x, yMax);
        ctx.lineTo(x + w, yMax);
        ctx.stroke();

        ctx.setLineDash([]);

        ctx.fillStyle = 'rgba(74, 222, 128, 0.9)';
        ctx.font = '9px Consolas, monospace';
        ctx.textAlign = 'left';
        ctx.textBaseline = 'bottom';
        ctx.fillText(`↑目标上限 ${Math.round(this.targetMax)}°C`, x + 3, yMax - 2);
        ctx.textBaseline = 'top';
        ctx.fillText(`↓目标下限 ${Math.round(this.targetMin)}°C`, x + 3, yMin + 2);
    }

    _drawGrid(ctx, x, y, w, h) {
        ctx.strokeStyle = 'rgba(107, 124, 147, 0.12)';
        ctx.lineWidth = 1;

        const yTicks = 5;
        for (let i = 0; i <= yTicks; i++) {
            const ty = y + (i / yTicks) * h;
            ctx.beginPath();
            ctx.moveTo(x, ty);
            ctx.lineTo(x + w, ty);
            ctx.stroke();
        }

        const xTicks = 4;
        for (let i = 0; i <= xTicks; i++) {
            const tx = x + (i / xTicks) * w;
            ctx.strokeStyle = i === 0 || i === xTicks
                ? 'rgba(107, 124, 147, 0.25)'
                : 'rgba(107, 124, 147, 0.08)';
            ctx.beginPath();
            ctx.moveTo(tx, y);
            ctx.lineTo(tx, y + h);
            ctx.stroke();
        }
    }

    _drawDataLine(ctx, x, y, w, h) {
        if (this.tempData.length < 2) return;

        const n = this.tempData.length;

        ctx.save();
        ctx.beginPath();
        for (let i = 0; i < n; i++) {
            const px = x + (i / (n - 1 || 1)) * w;
            const py = this._tempToY(this.tempData[i].temp, y, h);
            if (i === 0) {
                ctx.moveTo(px, py);
            } else {
                const prevPx = x + ((i - 1) / (n - 1)) * w;
                const prevPy = this._tempToY(this.tempData[i - 1].temp, y, h);
                const cpx1 = prevPx + (px - prevPx) * 0.5;
                const cpy1 = prevPy;
                const cpx2 = prevPx + (px - prevPx) * 0.5;
                const cpy2 = py;
                ctx.bezierCurveTo(cpx1, cpy1, cpx2, cpy2, px, py);
            }
        }

        const grad = ctx.createLinearGradient(x, y, x + w, y);
        grad.addColorStop(0, '#ff6b35');
        grad.addColorStop(0.5, '#ffc857');
        grad.addColorStop(1, '#ff6b35');

        ctx.strokeStyle = grad;
        ctx.lineWidth = 2.5;
        ctx.lineCap = 'round';
        ctx.lineJoin = 'round';
        ctx.stroke();

        ctx.shadowColor = '#ff6b35';
        ctx.shadowBlur = 8;
        ctx.strokeStyle = 'rgba(255, 107, 53, 0.3)';
        ctx.lineWidth = 4;
        ctx.stroke();
        ctx.shadowBlur = 0;

        ctx.lineTo(x + w, y + h);
        ctx.lineTo(x, y + h);
        ctx.closePath();
        const fillGrad = ctx.createLinearGradient(0, y, 0, y + h);
        fillGrad.addColorStop(0, 'rgba(255, 107, 53, 0.3)');
        fillGrad.addColorStop(1, 'rgba(255, 107, 53, 0)');
        ctx.fillStyle = fillGrad;
        ctx.fill();

        ctx.restore();
    }

    _drawDataPoints(ctx, x, y, w, h) {
        const n = this.tempData.length;
        if (n === 0) return;

        const highlightIdx = n - 1;

        for (let i = 0; i < n; i++) {
            const px = x + (i / (n - 1 || 1)) * w;
            const py = this._tempToY(this.tempData[i].temp, y, h);
            const isHighlight = i === highlightIdx;
            const isKeyPoint = i % Math.max(1, Math.floor(n / 6)) === 0 || isHighlight;

            if (!isKeyPoint && !isHighlight) continue;

            const size = isHighlight ? 5 : 2.5;
            const pulse = isHighlight ? 1 + Math.sin(this.animationTime * 4) * 0.2 : 1;

            if (isHighlight) {
                ctx.shadowColor = '#ff6b35';
                ctx.shadowBlur = 15;
            }

            const grad = ctx.createRadialGradient(px, py, 0, px, py, size * pulse * 2);
            grad.addColorStop(0, '#ffffff');
            grad.addColorStop(0.4, '#ffc857');
            grad.addColorStop(1, '#ff6b35');

            ctx.fillStyle = grad;
            ctx.beginPath();
            ctx.arc(px, py, size * pulse, 0, Math.PI * 2);
            ctx.fill();

            ctx.shadowBlur = 0;
        }
    }

    _drawAxis(ctx, x, y, w, h) {
        ctx.strokeStyle = 'rgba(155, 176, 199, 0.5)';
        ctx.lineWidth = 1.5;

        ctx.beginPath();
        ctx.moveTo(x, y + h);
        ctx.lineTo(x + w, y + h);
        ctx.stroke();

        ctx.beginPath();
        ctx.moveTo(x, y);
        ctx.lineTo(x, y + h);
        ctx.stroke();

        ctx.fillStyle = '#6b7c93';
        ctx.font = '10px Consolas, monospace';
        ctx.textAlign = 'right';
        ctx.textBaseline = 'middle';

        const yTicks = 5;
        for (let i = 0; i <= yTicks; i++) {
            const ty = y + (i / yTicks) * h;
            const temp = this.displayTempMax - (i / yTicks) * (this.displayTempMax - this.displayTempMin);
            ctx.fillText(`${Math.round(temp)}°`, x - 5, ty);

            ctx.strokeStyle = 'rgba(155, 176, 199, 0.3)';
            ctx.beginPath();
            ctx.moveTo(x - 3, ty);
            ctx.lineTo(x, ty);
            ctx.stroke();
        }

        ctx.fillStyle = '#6b7c93';
        ctx.font = '10px sans-serif';
        ctx.textAlign = 'center';
        ctx.textBaseline = 'top';

        const n = this.tempData.length;
        const xTicks = 3;
        for (let i = 0; i <= xTicks; i++) {
            const tx = x + (i / xTicks) * w;
            const dataIdx = Math.round((i / xTicks) * (n - 1 || 0));
            const elapsed = n > 1 ? -(n - 1 - dataIdx) * 10 : 0;
            const label = elapsed === 0 ? '现在' : `${Math.abs(elapsed)}s前`;
            ctx.fillText(label, tx, y + h + 8);
        }
    }

    _drawCurrentValue(ctx, x, y, w, h) {
        if (this.tempData.length === 0) return;

        const n = this.tempData.length;
        const current = this.tempData[n - 1].temp;
        const px = x + w;
        const py = this._tempToY(current, y, h);

        ctx.setLineDash([4, 4]);
        ctx.strokeStyle = 'rgba(255, 200, 87, 0.5)';
        ctx.lineWidth = 1;
        ctx.beginPath();
        ctx.moveTo(x, py);
        ctx.lineTo(px, py);
        ctx.stroke();
        ctx.setLineDash([]);

        const label = `${Math.round(current)}°C`;
        ctx.font = 'bold 11px Consolas, monospace';
        const metrics = ctx.measureText(label);
        const labelW = metrics.width + 14;
        const labelH = 20;
        const labelX = Math.min(x + w - labelW, px - 10 - labelW);
        const labelY = py - labelH / 2;

        ctx.fillStyle = 'rgba(255, 107, 53, 0.95)';
        this._roundRect(ctx, labelX, labelY, labelW, labelH, 4);
        ctx.fill();

        ctx.strokeStyle = '#ffaa66';
        ctx.lineWidth = 1;
        ctx.stroke();

        ctx.fillStyle = '#ffffff';
        ctx.textAlign = 'center';
        ctx.textBaseline = 'middle';
        ctx.fillText(label, labelX + labelW / 2, labelY + labelH / 2);
    }

    _tempToY(temp, y, h) {
        const range = this.displayTempMax - this.displayTempMin;
        const normalized = Math.max(0, Math.min(1, (temp - this.displayTempMin) / range));
        return y + (1 - normalized) * h;
    }

    _roundRect(ctx, x, y, w, h, r) {
        ctx.beginPath();
        ctx.moveTo(x + r, y);
        ctx.lineTo(x + w - r, y);
        ctx.quadraticCurveTo(x + w, y, x + w, y + r);
        ctx.lineTo(x + w, y + h - r);
        ctx.quadraticCurveTo(x + w, y + h, x + w - r, y + h);
        ctx.lineTo(x + r, y + h);
        ctx.quadraticCurveTo(x, y + h, x, y + h - r);
        ctx.lineTo(x, y + r);
        ctx.quadraticCurveTo(x, y, x + r, y);
        ctx.closePath();
    }
}

export default TemperatureChart;
