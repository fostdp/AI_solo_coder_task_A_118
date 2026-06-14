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

        this._isMobile = this._detectMobile();
        this._compressFactor = this._isMobile ? 4 : 2;

        this._offscreenCanvas = document.createElement('canvas');
        this._offscreenCtx = this._offscreenCanvas.getContext('2d');
        this._compressedData = null;
        this._lastDataHash = '';

        this._frameCounter = 0;
        this._drawSkip = this._isMobile ? 2 : 1;

        this.resizeCanvas();
        this.generateDefaultField();

        this._animate = this._animate.bind(this);
        window.addEventListener('resize', () => {
            this._isMobile = this._detectMobile();
            this._compressFactor = this._isMobile ? 4 : 2;
            this._drawSkip = this._isMobile ? 2 : 1;
            this.resizeCanvas();
        });
        this._animate();
    }

    _detectMobile() {
        const ua = navigator.userAgent || '';
        const isTouch = 'ontouchstart' in window;
        const isSmall = window.innerWidth < 768 || window.innerHeight < 600;
        return /Android|webOS|iPhone|iPad|iPod|BlackBerry|IEMobile|Opera Mini/i.test(ua)
            || (isTouch && isSmall);
    }

    resizeCanvas() {
        const rect = this.canvas.getBoundingClientRect();
        const dpr = this._isMobile ? 1 : Math.min(window.devicePixelRatio || 1, 1.5);
        this.canvas.width = Math.floor(rect.width * dpr);
        this.canvas.height = Math.floor(rect.height * dpr);
        this.ctx.setTransform(dpr, 0, 0, dpr, 0, 0);
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
        this._compressFieldData();
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
            this._compressFieldData();
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

    _compressFieldData() {
        if (!this.colorData || this.colorData.length === 0) return;
        const factor = this._compressFactor;
        const rows = this.resolution.rows;
        const cols = this.resolution.cols;
        const compRows = Math.max(4, Math.ceil(rows / factor));
        const compCols = Math.max(4, Math.ceil(cols / factor));

        const compressed = [];
        for (let cr = 0; cr < compRows; cr++) {
            compressed[cr] = [];
            for (let cc = 0; cc < compCols; cc++) {
                let rSum = 0, gSum = 0, bSum = 0, aSum = 0, count = 0;
                for (let dr = 0; dr < factor; dr++) {
                    for (let dc = 0; dc < factor; dc++) {
                        const r = cr * factor + dr;
                        const c = cc * factor + dc;
                        if (r < rows && c < cols && this.colorData[r][c]) {
                            const rgba = this._parseRgba(this.colorData[r][c]);
                            rSum += rgba.r;
                            gSum += rgba.g;
                            bSum += rgba.b;
                            aSum += rgba.a;
                            count++;
                        }
                    }
                }
                if (count > 0) {
                    compressed[cr][cc] = `rgba(${Math.round(rSum / count)},${Math.round(gSum / count)},${Math.round(bSum / count)},${(aSum / count).toFixed(2)})`;
                } else {
                    compressed[cr][cc] = 'rgba(0,0,0,0)';
                }
            }
        }

        this._compressedData = { rows: compRows, cols: compCols, pixels: compressed };
        this._hashCompressedData();
    }

    _parseRgba(str) {
        const m = str.match(/rgba?\((\d+),\s*(\d+),\s*(\d+)(?:,\s*([\d.]+))?\)/);
        if (m) {
            return {
                r: parseInt(m[1]),
                g: parseInt(m[2]),
                b: parseInt(m[3]),
                a: m[4] !== undefined ? parseFloat(m[4]) : 1
            };
        }
        return { r: 0, g: 0, b: 0, a: 0 };
    }

    _hashCompressedData() {
        if (!this._compressedData) { this._lastDataHash = ''; return; }
        let h = 0;
        const { rows, cols, pixels } = this._compressedData;
        const step = Math.max(1, Math.floor((rows * cols) / 64));
        for (let i = 0; i < rows * cols; i += step) {
            const r = Math.floor(i / cols);
            const c = i % cols;
            const px = pixels[r]?.[c] || '';
            for (let j = 0; j < px.length; j++) {
                h = ((h << 5) - h + px.charCodeAt(j)) | 0;
            }
        }
        this._lastDataHash = String(h);
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
        this._compressFieldData();
    }

    _interp(a, b, t) {
        return a + (b - a) * Math.max(0, Math.min(1, t));
    }

    _animate() {
        const now = performance.now();
        const dt = (now - this.lastFrameTime) / 1000;
        this.lastFrameTime = now;
        this.animationTime += dt;
        this._frameCounter++;

        if (this._frameCounter % this._drawSkip === 0) {
            this._draw();
        }

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
            this._drawTemperatureFieldOptimized(ctx, fieldX, fieldY, fieldW, fieldH);
        }

        this._drawZoneLabels(ctx, fieldX, fieldY, fieldW, fieldH);
        if (!this._isMobile) {
            this._drawIsotherms(ctx, fieldX, fieldY, fieldW, fieldH);
            this._drawFlowArrows(ctx, fieldX, fieldY, fieldW, fieldH);
        }
        this._drawColorBar(ctx, W - 60, fieldY, 25, fieldH);
        this._drawAxis(ctx, fieldX, fieldY, fieldW, fieldH);
    }

    _drawGridBackground(W, H) {
        const ctx = this.ctx;
        ctx.fillStyle = '#0a0f14';
        ctx.fillRect(0, 0, W, H);

        if (this._isMobile) return;

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

    _drawTemperatureFieldOptimized(ctx, x, y, w, h) {
        ctx.save();

        ctx.beginPath();
        ctx.moveTo(x + 5, y + h);
        ctx.lineTo(x + 18, y);
        ctx.lineTo(x + w - 18, y);
        ctx.lineTo(x + w - 5, y + h);
        ctx.closePath();
        ctx.clip();

        if (this._compressedData) {
            const { rows: cRows, cols: cCols, pixels } = this._compressedData;
            const off = this._offscreenCanvas;
            if (off.width !== cCols || off.height !== cRows) {
                off.width = cCols;
                off.height = cRows;
            }
            const octx = this._offscreenCtx;
            const imgData = octx.createImageData(cCols, cRows);
            const buf = imgData.data;

            for (let r = 0; r < cRows; r++) {
                for (let c = 0; c < cCols; c++) {
                    const rgba = this._parseRgba(pixels[r][c]);
                    const idx = (r * cCols + c) * 4;
                    buf[idx] = rgba.r;
                    buf[idx + 1] = rgba.g;
                    buf[idx + 2] = rgba.b;
                    buf[idx + 3] = Math.round(rgba.a * 255);
                }
            }
            octx.putImageData(imgData, 0, 0);

            ctx.imageSmoothingEnabled = true;
            ctx.imageSmoothingQuality = this._isMobile ? 'low' : 'medium';
            ctx.drawImage(off, x, y, w, h);
        } else {
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
        }

        ctx.restore();
    }

    _drawTemperatureField(ctx, x, y, w, h) {
        this._drawTemperatureFieldOptimized(ctx, x, y, w, h);
    }

    _drawZoneLabels(ctx, x, y, w, h) {
        const zoneNames = ['炉顶区', '预热区', '还原区', '熔化区', '炉缸区'];
        const zoneYs = [0.1, 0.3, 0.52, 0.75, 0.92];

        ctx.font = 'bold 11px sans-serif';
        ctx.textAlign = 'right';
        ctx.textBaseline = 'middle';

        for (let i = 0; i < 5; i++) {
            const yy = y + h * zoneYs[i];
            const temp = this.zones[i];
            ctx.fillStyle = 'rgba(255, 220, 150, 0.9)';
            ctx.fillText(`${zoneNames[i]} ${Math.round(temp)}°C`, x - 8, yy);

            ctx.strokeStyle = 'rgba(255, 220, 150, 0.15)';
            ctx.setLineDash([3, 4]);
            ctx.beginPath();
            ctx.moveTo(x, yy);
            ctx.lineTo(x + w, yy);
            ctx.stroke();
            ctx.setLineDash([]);
        }
    }

    _drawIsotherms(ctx, x, y, w, h) {
        const temps = [600, 800, 1000, 1200, 1400];
        const rows = this.resolution.rows;
        const cols = this.resolution.cols;

        ctx.lineWidth = 1;
        ctx.font = '10px sans-serif';

        for (const target of temps) {
            ctx.strokeStyle = this._tempToRgba(target).replace(/[\d.]+\)$/, '0.6)');
            ctx.setLineDash([2, 3]);
            ctx.beginPath();
            let started = false;

            for (let r = 1; r < rows; r += 2) {
                for (let c = 1; c < cols; c += 2) {
                    if (!this.fieldData[r]) continue;
                    const v00 = this.fieldData[r][c];
                    if (v00 === undefined) continue;
                    if ((v00 >= target && this.fieldData[r - 1]?.[c - 1] < target) ||
                        (v00 < target && this.fieldData[r - 1]?.[c - 1] >= target)) {
                        const px = x + (c / cols) * w;
                        const py = y + (r / rows) * h;
                        if (!started) {
                            ctx.moveTo(px, py);
                            started = true;
                        } else {
                            ctx.lineTo(px, py);
                        }
                    }
                }
            }
            ctx.stroke();
            ctx.setLineDash([]);

            const labelX = x + w - 30;
            const norm = Math.max(0, Math.min(1, (target - this.tempMin) / (this.tempMax - this.tempMin)));
            const labelY = y + h * (1 - norm);
            ctx.fillStyle = 'rgba(255,255,255,0.7)';
            ctx.fillText(`${target}°`, labelX, labelY);
        }
    }

    _drawFlowArrows(ctx, x, y, w, h) {
        const positions = [
            { rx: 0.5, ry: 0.9, dx: 0, dy: -1 },
            { rx: 0.3, ry: 0.7, dx: 0.15, dy: -0.8 },
            { rx: 0.7, ry: 0.7, dx: -0.15, dy: -0.8 },
            { rx: 0.25, ry: 0.45, dx: 0.3, dy: -0.6 },
            { rx: 0.75, ry: 0.45, dx: -0.3, dy: -0.6 },
            { rx: 0.5, ry: 0.15, dx: 0, dy: 1 },
        ];

        for (const p of positions) {
            const ax = x + p.rx * w;
            const ay = y + p.ry * h;
            const len = 12;
            const angle = Math.atan2(p.dy, p.dx) + Math.sin(this.animationTime + p.rx * 10) * 0.2;
            const ex = ax + Math.cos(angle) * len;
            const ey = ay + Math.sin(angle) * len;

            ctx.strokeStyle = 'rgba(255, 200, 100, 0.35)';
            ctx.lineWidth = 1.5;
            ctx.beginPath();
            ctx.moveTo(ax, ay);
            ctx.lineTo(ex, ey);
            ctx.stroke();

            ctx.beginPath();
            ctx.moveTo(ex, ey);
            ctx.lineTo(ex - Math.cos(angle - 0.4) * 5, ey - Math.sin(angle - 0.4) * 5);
            ctx.moveTo(ex, ey);
            ctx.lineTo(ex - Math.cos(angle + 0.4) * 5, ey - Math.sin(angle + 0.4) * 5);
            ctx.stroke();
        }
    }

    _drawColorBar(ctx, x, y, w, h) {
        const steps = 64;
        for (let i = 0; i < steps; i++) {
            const t = i / (steps - 1);
            const temp = this.tempMin + t * (this.tempMax - this.tempMin);
            ctx.fillStyle = this._tempToRgba(temp);
            const rectY = y + h * (1 - t);
            ctx.fillRect(x, rectY - 1, w, h / steps + 1);
        }

        ctx.strokeStyle = 'rgba(255,255,255,0.3)';
        ctx.lineWidth = 1;
        ctx.strokeRect(x, y, w, h);

        ctx.fillStyle = 'rgba(255,255,255,0.8)';
        ctx.font = '10px sans-serif';
        ctx.textAlign = 'left';
        ctx.textBaseline = 'middle';
        ctx.fillText(`${Math.round(this.tempMin)}°C`, x + w + 5, y + h);
        ctx.fillText(`${Math.round(this.tempMax)}°C`, x + w + 5, y);
        ctx.fillText(`${Math.round((this.tempMin + this.tempMax) / 2)}°C`, x + w + 5, y + h / 2);
    }

    _drawAxis(ctx, x, y, w, h) {
        ctx.strokeStyle = 'rgba(255,255,255,0.2)';
        ctx.lineWidth = 1;
        ctx.beginPath();
        ctx.moveTo(x, y + h + 5);
        ctx.lineTo(x + w, y + h + 5);
        ctx.moveTo(x - 5, y);
        ctx.lineTo(x - 5, y + h);
        ctx.stroke();

        ctx.fillStyle = 'rgba(255,255,255,0.5)';
        ctx.font = '9px sans-serif';
        ctx.textAlign = 'center';
        ctx.fillText('径向 →', x + w / 2, y + h + 18);

        ctx.save();
        ctx.translate(x - 28, y + h / 2);
        ctx.rotate(-Math.PI / 2);
        ctx.textAlign = 'center';
        ctx.fillText('高度 ↑ (炉缸→炉顶)', 0, 0);
        ctx.restore();
    }

    _tempToRgba(temp) {
        const t = Math.max(0, Math.min(1, (temp - this.tempMin) / (this.tempMax - this.tempMin)));

        if (t < 0.2) {
            const k = t / 0.2;
            return `rgba(20,${Math.round(80 + 100 * k)},${Math.round(120 + 80 * k)},0.92)`;
        } else if (t < 0.4) {
            const k = (t - 0.2) / 0.2;
            return `rgba(${Math.round(40 + 100 * k)},${Math.round(180 + 30 * k)},${Math.round(200 - 150 * k)},0.92)`;
        } else if (t < 0.6) {
            const k = (t - 0.4) / 0.2;
            return `rgba(${Math.round(140 + 100 * k)},${Math.round(210 - 30 * k)},${Math.round(50 - 50 * k)},0.92)`;
        } else if (t < 0.8) {
            const k = (t - 0.6) / 0.2;
            return `rgba(${Math.round(240 - 10 * k)},${Math.round(180 - 80 * k)},0,0.92)`;
        } else {
            const k = (t - 0.8) / 0.2;
            return `rgba(${Math.round(230 + 25 * k)},${Math.round(100 + 50 * k)},${Math.round(0 + 80 * k)},0.92)`;
        }
    }
}
