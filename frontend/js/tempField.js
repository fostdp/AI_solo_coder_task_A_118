export class TempFieldVisualization {
    constructor(canvasId) {
        this.canvas = document.getElementById(canvasId);
        if (!this.canvas) {
            throw new Error(`Canvas ${canvasId} not found`);
        }
        this.ctx = this.canvas.getContext('2d');

        this.tempMin = 400;
        this.tempMax = 1600;
        this.resolution = { rows: 64, cols: 192 };
        this.zones = [800, 950, 1100, 1250, 1350];
        this.fieldData = null;
        this.colorData = null;
        this.animationTime = 0;
        this.lastFrameTime = performance.now();

        this.resizeCanvas();
        this.generateDefaultField();

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

    generateDefaultField() {
        const { rows, cols } = this.resolution;
        this.fieldData = [];
        this.colorData = [];

        for (let r = 0; r < rows; r++) {
            this.fieldData[r] = [];
            this.colorData[r] = [];
            const ry = r / (rows - 1);
            let baseTemp;

            if (ry < 0.2) {
                const t = ry / 0.2;
                baseTemp = this.zones[0] + (this.zones[1] - this.zones[0]) * t;
            } else if (ry < 0.4) {
                const t = (ry - 0.2) / 0.2;
                baseTemp = this.zones[1] + (this.zones[2] - this.zones[1]) * t;
            } else if (ry < 0.65) {
                const t = (ry - 0.4) / 0.25;
                baseTemp = this.zones[2] + (this.zones[3] - this.zones[2]) * t;
            } else if (ry < 0.85) {
                const t = (ry - 0.65) / 0.2;
                baseTemp = this.zones[3] + (this.zones[4] - this.zones[3]) * t;
            } else {
                const t = (ry - 0.85) / 0.15;
                baseTemp = this.zones[4] + 30 * (1 - t);
            }

            for (let c = 0; c < cols; c++) {
                const cx = (c / (cols - 1)) - 0.5;
                const radial = Math.abs(cx) * 2;
                const edgeFactor = 1 - radial * 0.25;
                const noise = Math.sin(ry * 30 + cx * 15) * 12
                    + Math.cos(ry * 20 - cx * 25) * 8
                    + (Math.random() - 0.5) * 6;

                const temp = baseTemp * edgeFactor + noise;
                this.fieldData[r][c] = temp;
                this.colorData[r][c] = this._tempToRgba(temp);
            }
        }
    }

    updateData(apiData) {
        if (apiData.temp_min !== undefined) this.tempMin = apiData.temp_min;
        if (apiData.temp_max !== undefined) this.tempMax = apiData.temp_max;
        if (apiData.zones) this.zones = apiData.zones;

        if (apiData.field_data && apiData.color_data) {
            this.fieldData = apiData.field_data;
            this.resolution = {
                rows: this.fieldData.length,
                cols: this.fieldData[0]?.length || this.resolution.cols
            };
            this.colorData = apiData.color_data;
        } else if (apiData.zones) {
            this._regenerateFromZones();
        }

        if (apiData.temp_min !== undefined) {
            const elMin = document.getElementById('tempScaleMin');
            if (elMin) elMin.textContent = `${Math.round(this.tempMin)}°C`;
        }
        if (apiData.temp_max !== undefined) {
            const elMax = document.getElementById('tempScaleMax');
            if (elMax) elMax.textContent = `${Math.round(this.tempMax)}°C`;
        }
    }

    _regenerateFromZones() {
        const { rows, cols } = this.resolution;
        this.fieldData = [];
        this.colorData = [];

        for (let r = 0; r < rows; r++) {
            this.fieldData[r] = [];
            this.colorData[r] = [];
            const ry = r / (rows - 1);
            let baseTemp;

            if (ry < 0.2) {
                baseTemp = this._interp(this.zones[0], this.zones[1], ry / 0.2);
            } else if (ry < 0.4) {
                baseTemp = this._interp(this.zones[1], this.zones[2], (ry - 0.2) / 0.2);
            } else if (ry < 0.65) {
                baseTemp = this._interp(this.zones[2], this.zones[3], (ry - 0.4) / 0.25);
            } else if (ry < 0.85) {
                baseTemp = this._interp(this.zones[3], this.zones[4], (ry - 0.65) / 0.2);
            } else {
                baseTemp = this._interp(this.zones[4] + 30, this.zones[4], (ry - 0.85) / 0.15);
            }

            for (let c = 0; c < cols; c++) {
                const cx = (c / (cols - 1)) - 0.5;
                const radial = Math.abs(cx) * 2;
                const edgeFactor = 1 - radial * 0.25;
                const noise = Math.sin(ry * 30 + cx * 15 + this.animationTime * 0.5) * 10
                    + Math.cos(ry * 20 - cx * 25 + this.animationTime * 0.3) * 6;

                const temp = baseTemp * edgeFactor + noise;
                this.fieldData[r][c] = temp;
                this.colorData[r][c] = this._tempToRgba(temp);
            }
        }
    }

    _interp(a, b, t) {
        return a + (b - a) * Math.max(0, Math.min(1, t));
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

        this._drawGridBackground(W, H);

        const padding = { top: 20, right: 80, bottom: 30, left: 50 };
        const fieldX = padding.left;
        const fieldY = padding.top;
        const fieldW = W - padding.left - padding.right;
        const fieldH = H - padding.top - padding.bottom;

        this._drawFurnaceOutline(ctx, fieldX, fieldY, fieldW, fieldH);

        if (this.colorData && this.colorData.length > 0) {
            this._drawTemperatureField(ctx, fieldX, fieldY, fieldW, fieldH);
        }

        this._drawZoneLabels(ctx, fieldX, fieldY, fieldW, fieldH);
        this._drawIsotherms(ctx, fieldX, fieldY, fieldW, fieldH);
        this._drawFlowArrows(ctx, fieldX, fieldY, fieldW, fieldH);
        this._drawColorBar(ctx, W - 60, fieldY, 25, fieldH);
        this._drawAxis(ctx, fieldX, fieldY, fieldW, fieldH);
    }

    _drawGridBackground(W, H) {
        const ctx = this.ctx;
        ctx.fillStyle = '#0a0f14';
        ctx.fillRect(0, 0, W, H);

        ctx.strokeStyle = 'rgba(78, 205, 196, 0.04)';
        ctx.lineWidth = 1;
        for (let x = 0; x < W; x += 25) {
            ctx.beginPath();
            ctx.moveTo(x, 0);
            ctx.lineTo(x, H);
            ctx.stroke();
        }
        for (let y = 0; y < H; y += 25) {
            ctx.beginPath();
            ctx.moveTo(0, y);
            ctx.lineTo(W, y);
            ctx.stroke();
        }
    }

    _drawFurnaceOutline(ctx, x, y, w, h) {
        const grad = ctx.createLinearGradient(x, y, x + w, y);
        grad.addColorStop(0, '#8b6514');
        grad.addColorStop(0.5, '#a07516');
        grad.addColorStop(1, '#8b6514');

        ctx.strokeStyle = grad;
        ctx.lineWidth = 4;
        ctx.beginPath();
        ctx.moveTo(x + 5, y + h);
        ctx.lineTo(x + 18, y);
        ctx.lineTo(x + w - 18, y);
        ctx.lineTo(x + w - 5, y + h);
        ctx.closePath();
        ctx.stroke();

        ctx.strokeStyle = 'rgba(139, 101, 20, 0.3)';
        ctx.lineWidth = 8;
        ctx.stroke();
    }

    _drawTemperatureField(ctx, x, y, w, h) {
        ctx.save();

        ctx.beginPath();
        ctx.moveTo(x + 5, y + h);
        ctx.lineTo(x + 18, y);
        ctx.lineTo(x + w - 18, y);
        ctx.lineTo(x + w - 5, y + h);
        ctx.closePath();
        ctx.clip();

        const rows = this.resolution.rows;
        const cols = this.resolution.cols;
        const pixelW = w / cols;
        const pixelH = h / rows;

        for (let r = 0; r < rows; r++) {
            for (let c = 0; c < cols; c++) {
                const px = x + c * pixelW;
                const py = y + r * pixelH;
                ctx.fillStyle = this.colorData[r][c];
                ctx.fillRect(px, py, pixelW + 0.5, pixelH + 0.5);
            }
        }

        ctx.globalCompositeOperation = 'overlay';
        const time = this.animationTime;
        for (let i = 0; i < 8; i++) {
            const waveY = y + ((time * 20 + i * 30) % h);
            const waveGrad = ctx.createLinearGradient(0, waveY - 30, 0, waveY + 30);
            waveGrad.addColorStop(0, 'rgba(255, 200, 100, 0)');
            waveGrad.addColorStop(0.5, 'rgba(255, 200, 100, 0.15)');
            waveGrad.addColorStop(1, 'rgba(255, 200, 100, 0)');
            ctx.fillStyle = waveGrad;
            ctx.fillRect(x, waveY - 30, w, 60);
        }

        ctx.globalCompositeOperation = 'source-over';
        ctx.restore();
    }

    _drawIsotherms(ctx, x, y, w, h) {
        const tempSteps = [600, 800, 1000, 1200, 1400];
        ctx.save();

        ctx.beginPath();
        ctx.moveTo(x + 5, y + h);
        ctx.lineTo(x + 18, y);
        ctx.lineTo(x + w - 18, y);
        ctx.lineTo(x + w - 5, y + h);
        ctx.closePath();
        ctx.clip();

        tempSteps.forEach(temp => {
            if (temp < this.tempMin || temp > this.tempMax) return;

            ctx.strokeStyle = `rgba(255, 255, 255, 0.25)`;
            ctx.lineWidth = 1;
            ctx.setLineDash([4, 4]);
            ctx.beginPath();

            const rows = this.resolution.rows;
            const cols = this.resolution.cols;
            let started = false;

            for (let c = 0; c < cols; c++) {
                let foundRow = -1;
                for (let r = 0; r < rows - 1; r++) {
                    const t1 = this.fieldData[r][c];
                    const t2 = this.fieldData[r + 1][c];
                    if ((t1 - temp) * (t2 - temp) <= 0) {
                        const interp = (temp - t1) / (t2 - t1 || 1);
                        foundRow = r + interp;
                        break;
                    }
                }
                if (foundRow >= 0) {
                    const px = x + (c / (cols - 1)) * w;
                    const py = y + (foundRow / (rows - 1)) * h;
                    if (!started) {
                        ctx.moveTo(px, py);
                        started = true;
                    } else {
                        ctx.lineTo(px, py);
                    }
                }
            }

            ctx.stroke();
            ctx.setLineDash([]);
        });

        ctx.restore();
    }

    _drawFlowArrows(ctx, x, y, w, h) {
        ctx.save();
        ctx.globalAlpha = 0.4 + 0.2 * Math.sin(this.animationTime * 2);
        ctx.strokeStyle = '#ffffff';
        ctx.fillStyle = '#ffffff';
        ctx.lineWidth = 1.5;

        const arrows = [
            { px: 0.5, py: 0.9, dx: 0, dy: -1 },
            { px: 0.3, py: 0.75, dx: 0.1, dy: -0.8 },
            { px: 0.7, py: 0.75, dx: -0.1, dy: -0.8 },
            { px: 0.25, py: 0.5, dx: 0.2, dy: -0.6 },
            { px: 0.75, py: 0.5, dx: -0.2, dy: -0.6 },
            { px: 0.5, py: 0.3, dx: 0, dy: -0.4 },
        ];

        arrows.forEach(arr => {
            const ax = x + arr.px * w;
            const ay = y + arr.py * h;
            const len = 18 + 5 * Math.sin(this.animationTime * 3 + arr.px * 10);

            ctx.beginPath();
            ctx.moveTo(ax, ay);
            ctx.lineTo(ax + arr.dx * len, ay + arr.dy * len);
            ctx.stroke();

            const angle = Math.atan2(arr.dy, arr.dx);
            const headLen = 6;
            ctx.beginPath();
            ctx.moveTo(ax + arr.dx * len, ay + arr.dy * len);
            ctx.lineTo(
                ax + arr.dx * len - headLen * Math.cos(angle - Math.PI / 6),
                ay + arr.dy * len - headLen * Math.sin(angle - Math.PI / 6)
            );
            ctx.lineTo(
                ax + arr.dx * len - headLen * Math.cos(angle + Math.PI / 6),
                ay + arr.dy * len - headLen * Math.sin(angle + Math.PI / 6)
            );
            ctx.closePath();
            ctx.fill();
        });

        ctx.restore();
    }

    _drawZoneLabels(ctx, x, y, w, h) {
        const zones = [
            { name: '炉顶', py: 0.08, temp: this.zones[0] },
            { name: '上部', py: 0.3, temp: this.zones[1] },
            { name: '中部', py: 0.52, temp: this.zones[2] },
            { name: '下部', py: 0.74, temp: this.zones[3] },
            { name: '炉缸', py: 0.92, temp: this.zones[4] },
        ];

        ctx.font = '11px sans-serif';
        zones.forEach(z => {
            const zy = y + z.py * h;

            ctx.fillStyle = 'rgba(15, 25, 40, 0.7)';
            const labelW = 72;
            const labelH = 20;
            ctx.fillRect(x - labelW - 8, zy - labelH / 2, labelW, labelH);

            ctx.fillStyle = '#9bb0c7';
            ctx.textAlign = 'center';
            ctx.textBaseline = 'middle';
            ctx.fillText(z.name, x - labelW / 2 - 8, zy - 6);

            ctx.fillStyle = '#ffc857';
            ctx.font = 'bold 11px Consolas, monospace';
            ctx.fillText(`${Math.round(z.temp)}°C`, x - labelW / 2 - 8, zy + 7);

            ctx.strokeStyle = 'rgba(255, 255, 255, 0.15)';
            ctx.lineWidth = 1;
            ctx.setLineDash([2, 4]);
            ctx.beginPath();
            ctx.moveTo(x - 8, zy);
            ctx.lineTo(x + 8, zy);
            ctx.stroke();
            ctx.setLineDash([]);

            ctx.font = '11px sans-serif';
        });
    }

    _drawColorBar(ctx, x, y, w, h) {
        const steps = 50;
        for (let i = 0; i < steps; i++) {
            const t = 1 - (i / (steps - 1));
            const temp = this.tempMin + t * (this.tempMax - this.tempMin);
            ctx.fillStyle = this._tempToRgba(temp);
            ctx.fillRect(x, y + (i / steps) * h, w, h / steps + 1);
        }

        ctx.strokeStyle = 'rgba(255, 255, 255, 0.3)';
        ctx.lineWidth = 1;
        ctx.strokeRect(x, y, w, h);

        ctx.fillStyle = '#9bb0c7';
        ctx.font = '10px Consolas, monospace';
        ctx.textAlign = 'left';
        ctx.textBaseline = 'middle';

        const labels = 5;
        for (let i = 0; i < labels; i++) {
            const t = i / (labels - 1);
            const temp = this.tempMax - t * (this.tempMax - this.tempMin);
            const ly = y + t * h;
            ctx.fillText(`${Math.round(temp)}°`, x + w + 4, ly);

            ctx.strokeStyle = 'rgba(255, 255, 255, 0.4)';
            ctx.beginPath();
            ctx.moveTo(x - 3, ly);
            ctx.lineTo(x, ly);
            ctx.stroke();
        }
    }

    _drawAxis(ctx, x, y, w, h) {
        ctx.strokeStyle = 'rgba(155, 176, 199, 0.4)';
        ctx.lineWidth = 1;
        ctx.fillStyle = '#6b7c93';
        ctx.font = '10px sans-serif';

        ctx.beginPath();
        ctx.moveTo(x, y + h + 10);
        ctx.lineTo(x + w, y + h + 10);
        ctx.stroke();

        ctx.fillStyle = '#6b7c93';
        ctx.textAlign = 'center';
        ctx.fillText('炉径向 →', x + w / 2, y + h + 22);

        const xticks = 5;
        for (let i = 0; i < xticks; i++) {
            const tx = x + (i / (xticks - 1)) * w;
            ctx.strokeStyle = 'rgba(155, 176, 199, 0.3)';
            ctx.beginPath();
            ctx.moveTo(tx, y + h + 10);
            ctx.lineTo(tx, y + h + 14);
            ctx.stroke();
        }
    }

    _tempToRgba(temp) {
        const t = Math.max(0, Math.min(1, (temp - this.tempMin) / (this.tempMax - this.tempMin)));
        let r, g, b, a = 1;

        if (t < 0.2) {
            const p = t / 0.2;
            r = 78 + p * 150;
            g = 205 - p * 40;
            b = 196 - p * 120;
        } else if (t < 0.4) {
            const p = (t - 0.2) / 0.2;
            r = 228 + p * 12;
            g = 165 + p * 35;
            b = 76 - p * 40;
        } else if (t < 0.6) {
            const p = (t - 0.4) / 0.2;
            r = 240 + p * 15;
            g = 200 + p * 0;
            b = 36 - p * 20;
        } else if (t < 0.8) {
            const p = (t - 0.6) / 0.2;
            r = 255;
            g = 200 - p * 50;
            b = 16 - p * 16;
        } else {
            const p = (t - 0.8) / 0.2;
            r = 255;
            g = 150 - p * 80;
            b = 0;
        }

        return `rgba(${Math.round(r)}, ${Math.round(g)}, ${Math.round(b)}, ${a})`;
    }

    _hexToRgba(hex) {
        const m = hex.replace('#', '').match(/.{2}/g);
        if (!m || m.length < 3) return 'rgba(255, 100, 0, 1)';
        const r = parseInt(m[0], 16);
        const g = parseInt(m[1], 16);
        const b = parseInt(m[2], 16);
        return `rgba(${r}, ${g}, ${b}, 1)`;
    }
}

export default TempFieldVisualization;
