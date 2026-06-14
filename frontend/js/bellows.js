export class BellowsAnimation {
    constructor(canvasId) {
        this.canvas = document.getElementById(canvasId);
        if (!this.canvas) {
            throw new Error(`Canvas ${canvasId} not found`);
        }
        this.ctx = this.canvas.getContext('2d');

        this.frequency = 30;
        this.stroke = 40;
        this.windPressure = 1000;
        this.airVolume = 0.5;
        this.phase = 0;
        this.animationTime = 0;
        this.lastFrameTime = performance.now();
        this.sparkParticles = [];

        this.resizeCanvas();
        this.initSparks();

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

    initSparks() {
        for (let i = 0; i < 25; i++) {
            this.sparkParticles.push({
                x: 0,
                y: 0,
                vx: 0,
                vy: 0,
                life: 0,
                maxLife: 0,
                size: 0,
                color: '#ff6b35'
            });
        }
    }

    updateData(frequency, stroke, pressure, airVolume) {
        this.frequency = frequency || this.frequency;
        this.stroke = stroke || this.stroke;
        this.windPressure = pressure || this.windPressure;
        this.airVolume = airVolume || this.airVolume;
    }

    updateStatus(statusEl) {
        const phases = ['推气', '吸气'];
        const idx = Math.floor(((this.phase / (Math.PI * 2)) * 2)) % 2;
        if (statusEl) {
            statusEl.textContent = phases[idx];
        }
    }

    _animate() {
        const now = performance.now();
        const dt = (now - this.lastFrameTime) / 1000;
        this.lastFrameTime = now;
        this.animationTime += dt;

        const freqHz = this.frequency / 60;
        this.phase += dt * Math.PI * 2 * freqHz;
        if (this.phase > Math.PI * 2) {
            this.phase -= Math.PI * 2;
        }

        this._draw(dt);

        requestAnimationFrame(this._animate);
    }

    _draw(dt) {
        const ctx = this.ctx;
        const W = this.displayWidth;
        const H = this.displayHeight;

        ctx.clearRect(0, 0, W, H);

        this._drawBackground(W, H);
        this._drawFloor(W, H);

        const centerY = H * 0.55;
        const strokeFactor = this.stroke / 40;
        const strokePx = strokeFactor * 60;

        const handleX = W * 0.15 + Math.sin(this.phase) * strokePx * 0.8;
        const bodyEndX = handleX + 30;
        const bodyStartX = bodyEndX + 100;
        const nozzleEndX = bodyStartX + 50;

        this._drawStand(W * 0.25, centerY + 30);

        const compressed = Math.sin(this.phase) > 0;
        this._drawBellowsBody(bodyEndX, centerY, bodyStartX - bodyEndX, compressed, strokeFactor);

        this._drawHandle(handleX, centerY);
        this._drawLinkage(handleX, centerY, bodyEndX);
        this._drawNozzle(nozzleEndX, centerY, bodyStartX, compressed);

        const furnaceX = W * 0.82;
        this._drawFurnaceCrossSection(furnaceX, centerY, W, H, compressed);

        if (compressed) {
            this._drawAirFlow(bodyStartX + 40, centerY, furnaceX - 60, centerY);
            this._updateSparks(furnaceX - 40, centerY, dt);
            this._drawSparks(furnaceX - 30, centerY);
        } else {
            this._drawAirFlowReverse(bodyEndX, centerY, handleX - 15, centerY);
        }

        this._drawInfoPanel(W, H, dt);
    }

    _drawBackground(W, H) {
        const ctx = this.ctx;
        const grad = ctx.createLinearGradient(0, 0, 0, H);
        grad.addColorStop(0, '#0d1520');
        grad.addColorStop(0.5, '#131d2b');
        grad.addColorStop(1, '#1a2738');
        ctx.fillStyle = grad;
        ctx.fillRect(0, 0, W, H);

        ctx.globalAlpha = 0.03;
        ctx.strokeStyle = '#4ecdc4';
        ctx.lineWidth = 1;
        for (let x = 0; x < W; x += 30) {
            ctx.beginPath();
            ctx.moveTo(x, 0);
            ctx.lineTo(x, H);
            ctx.stroke();
        }
        for (let y = 0; y < H; y += 30) {
            ctx.beginPath();
            ctx.moveTo(0, y);
            ctx.lineTo(W, y);
            ctx.stroke();
        }
        ctx.globalAlpha = 1;
    }

    _drawFloor(W, H) {
        const ctx = this.ctx;
        const floorY = H * 0.78;
        const grad = ctx.createLinearGradient(0, floorY, 0, H);
        grad.addColorStop(0, '#3a2e24');
        grad.addColorStop(1, '#1a1510');
        ctx.fillStyle = grad;
        ctx.fillRect(0, floorY, W, H - floorY);

        ctx.strokeStyle = '#5a4535';
        ctx.lineWidth = 1;
        for (let x = 0; x < W; x += 40) {
            ctx.beginPath();
            ctx.moveTo(x, floorY);
            ctx.lineTo(x, H);
            ctx.stroke();
        }
    }

    _drawStand(baseX, baseY) {
        const ctx = this.ctx;

        ctx.fillStyle = '#3a2818';
        ctx.strokeStyle = '#5a3820';
        ctx.lineWidth = 2;

        ctx.beginPath();
        ctx.rect(baseX - 35, baseY, 70, 8);
        ctx.fill();
        ctx.stroke();

        ctx.beginPath();
        ctx.moveTo(baseX - 30, baseY);
        ctx.lineTo(baseX - 20, baseY - 50);
        ctx.lineTo(baseX + 20, baseY - 50);
        ctx.lineTo(baseX + 30, baseY);
        ctx.closePath();
        ctx.fill();
        ctx.stroke();
    }

    _drawHandle(x, y) {
        const ctx = this.ctx;

        ctx.fillStyle = '#6b4525';
        ctx.strokeStyle = '#8b5a35';
        ctx.lineWidth = 2;

        ctx.beginPath();
        ctx.arc(x - 12, y, 14, 0, Math.PI * 2);
        ctx.fill();
        ctx.stroke();

        ctx.beginPath();
        ctx.arc(x - 12, y, 5, 0, Math.PI * 2);
        ctx.fillStyle = '#3a2515';
        ctx.fill();

        ctx.fillStyle = '#5a3820';
        ctx.beginPath();
        ctx.rect(x, y - 5, 35, 10);
        ctx.fill();
        ctx.strokeStyle = '#7a4828';
        ctx.stroke();
    }

    _drawLinkage(handleX, y, bodyX) {
        const ctx = this.ctx;

        ctx.strokeStyle = '#4a3828';
        ctx.lineWidth = 8;
        ctx.lineCap = 'round';
        ctx.beginPath();
        ctx.moveTo(handleX + 22, y);
        ctx.lineTo(bodyX, y);
        ctx.stroke();

        ctx.fillStyle = '#8b6535';
        const rivets = 4;
        for (let i = 0; i < rivets; i++) {
            const rx = handleX + 22 + ((bodyX - handleX - 22) / (rivets - 1)) * i;
            ctx.beginPath();
            ctx.arc(rx, y, 4, 0, Math.PI * 2);
            ctx.fill();
        }
    }

    _drawBellowsBody(x, y, width, compressed, strokeFactor) {
        const ctx = this.ctx;
        const height = 90;
        const folds = 6;
        const compression = compressed ? 0.65 : 1.0;
        const actualWidth = width * compression;

        const bodyGrad = ctx.createLinearGradient(x, y - height / 2, x, y + height / 2);
        bodyGrad.addColorStop(0, '#c89a5a');
        bodyGrad.addColorStop(0.5, '#a07530');
        bodyGrad.addColorStop(1, '#7a5020');

        ctx.fillStyle = bodyGrad;
        ctx.strokeStyle = '#5a3815';
        ctx.lineWidth = 2;

        ctx.beginPath();
        ctx.moveTo(x, y - height / 2);

        const foldWidth = actualWidth / folds;
        for (let i = 0; i < folds; i++) {
            const fx = x + i * foldWidth;
            const wobble = (i % 2 === 0 ? -1 : 1) * 8 * (compressed ? 1.3 : 1.0);

            ctx.quadraticCurveTo(
                fx + foldWidth / 2,
                y - height / 2 + wobble,
                fx + foldWidth,
                y - height / 2
            );
        }

        ctx.lineTo(x + actualWidth, y + height / 2);

        for (let i = folds - 1; i >= 0; i--) {
            const fx = x + i * foldWidth;
            const wobble = (i % 2 === 0 ? 1 : -1) * 8 * (compressed ? 1.3 : 1.0);

            ctx.quadraticCurveTo(
                fx + foldWidth / 2,
                y + height / 2 + wobble,
                fx,
                y + height / 2
            );
        }

        ctx.closePath();
        ctx.fill();
        ctx.stroke();

        ctx.strokeStyle = 'rgba(90, 56, 21, 0.4)';
        ctx.lineWidth = 1;
        for (let i = 1; i < folds; i++) {
            const fx = x + i * foldWidth;
            ctx.beginPath();
            ctx.moveTo(fx, y - height / 2 + 5);
            ctx.lineTo(fx, y + height / 2 - 5);
            ctx.stroke();
        }

        const endPlateGrad = ctx.createLinearGradient(x + actualWidth, y - height / 2, x + actualWidth + 12, y);
        endPlateGrad.addColorStop(0, '#4a3020');
        endPlateGrad.addColorStop(1, '#6b4528');
        ctx.fillStyle = endPlateGrad;
        ctx.strokeStyle = '#3a2010';
        ctx.lineWidth = 2;
        ctx.beginPath();
        ctx.rect(x + actualWidth, y - height / 2 - 5, 12, height + 10);
        ctx.fill();
        ctx.stroke();
    }

    _drawNozzle(endX, y, bodyX, compressed) {
        const ctx = this.ctx;

        const nozzleGrad = ctx.createLinearGradient(bodyX, y - 20, endX, y);
        nozzleGrad.addColorStop(0, '#6b5540');
        nozzleGrad.addColorStop(1, '#4a3525');

        ctx.fillStyle = nozzleGrad;
        ctx.strokeStyle = '#3a2515';
        ctx.lineWidth = 2;

        ctx.beginPath();
        ctx.moveTo(bodyX + 12, y - 35);
        ctx.lineTo(endX, y - 18);
        ctx.lineTo(endX, y + 18);
        ctx.lineTo(bodyX + 12, y + 35);
        ctx.closePath();
        ctx.fill();
        ctx.stroke();

        ctx.fillStyle = compressed ? '#ffaa66' : '#2a2520';
        ctx.beginPath();
        ctx.ellipse(endX, y, 4, 18, 0, 0, Math.PI * 2);
        ctx.fill();

        if (compressed) {
            ctx.shadowColor = '#ff6b35';
            ctx.shadowBlur = 20;
            ctx.fillStyle = '#ff8844';
            ctx.beginPath();
            ctx.ellipse(endX, y, 2, 10, 0, 0, Math.PI * 2);
            ctx.fill();
            ctx.shadowBlur = 0;
        }
    }

    _drawFurnaceCrossSection(x, y, W, H, compressed) {
        const ctx = this.ctx;
        const furnaceWidth = 130;
        const furnaceHeight = 240;

        ctx.fillStyle = '#5a4530';
        ctx.strokeStyle = '#3a2515';
        ctx.lineWidth = 3;

        ctx.beginPath();
        ctx.moveTo(x - furnaceWidth / 2 - 15, y + furnaceHeight / 2 + 20);
        ctx.lineTo(x + furnaceWidth / 2 + 15, y + furnaceHeight / 2 + 20);
        ctx.lineTo(x + furnaceWidth / 2 + 10, y + furnaceHeight / 2 + 5);
        ctx.lineTo(x - furnaceWidth / 2 - 10, y + furnaceHeight / 2 + 5);
        ctx.closePath();
        ctx.fill();
        ctx.stroke();

        const wallGrad = ctx.createLinearGradient(x - furnaceWidth / 2, y, x + furnaceWidth / 2, y);
        wallGrad.addColorStop(0, '#8b6514');
        wallGrad.addColorStop(0.15, '#7a5510');
        wallGrad.addColorStop(0.85, '#7a5510');
        wallGrad.addColorStop(1, '#8b6514');

        ctx.fillStyle = wallGrad;
        ctx.beginPath();
        ctx.moveTo(x - furnaceWidth / 2, y + furnaceHeight / 2);
        ctx.lineTo(x - furnaceWidth / 2 + 10, y - furnaceHeight / 2);
        ctx.lineTo(x + furnaceWidth / 2 - 10, y - furnaceHeight / 2);
        ctx.lineTo(x + furnaceWidth / 2, y + furnaceHeight / 2);
        ctx.closePath();
        ctx.fill();
        ctx.stroke();

        const innerTemp = this.tempData || 1200;
        const zones = [
            { y: 0.85, temp: innerTemp + 40, label: '炉缸' },
            { y: 0.65, temp: innerTemp + 10, label: '下部' },
            { y: 0.45, temp: innerTemp - 20, label: '中部' },
            { y: 0.25, temp: innerTemp - 80, label: '上部' },
            { y: 0.08, temp: innerTemp - 150, label: '炉顶' }
        ];

        zones.forEach((zone, idx) => {
            const top = y - furnaceHeight / 2 + zone.y * furnaceHeight;
            const bottom = (idx < zones.length - 1)
                ? y - furnaceHeight / 2 + zones[idx + 1].y * furnaceHeight
                : y + furnaceHeight / 2 - 5;
            const h = bottom - top;

            const tempColor = this._tempToHex(zone.temp);

            ctx.save();
            ctx.beginPath();
            const lx1 = x - furnaceWidth / 2 + 12 + idx * 1.5;
            const rx1 = x + furnaceWidth / 2 - 12 - idx * 1.5;
            const lx2 = x - furnaceWidth / 2 + 14 + (idx + 1) * 1.5;
            const rx2 = x + furnaceWidth / 2 - 14 - (idx + 1) * 1.5;
            ctx.moveTo(lx1, top);
            ctx.lineTo(rx1, top);
            ctx.lineTo(rx2, bottom);
            ctx.lineTo(lx2, bottom);
            ctx.closePath();
            ctx.clip();

            ctx.globalAlpha = 0.85;
            ctx.fillStyle = tempColor;
            ctx.fillRect(x - furnaceWidth / 2, top, furnaceWidth, h);

            const heatGrad = ctx.createRadialGradient(x, (top + bottom) / 2, 0, x, (top + bottom) / 2, furnaceWidth / 2);
            heatGrad.addColorStop(0, 'rgba(255, 255, 255, 0.3)');
            heatGrad.addColorStop(1, 'rgba(255, 255, 255, 0)');
            ctx.fillStyle = heatGrad;
            ctx.fillRect(x - furnaceWidth / 2, top, furnaceWidth, h);

            ctx.restore();
        });

        const doorGrad = ctx.createLinearGradient(x - 12, y + 50, x + 12, y + 50);
        doorGrad.addColorStop(0, '#2a1508');
        doorGrad.addColorStop(0.5, compressed ? '#ff6b35' : '#1a0a05');
        doorGrad.addColorStop(1, '#2a1508');
        ctx.fillStyle = doorGrad;
        ctx.strokeStyle = '#5a3015';
        ctx.lineWidth = 2;
        ctx.beginPath();
        ctx.rect(x - furnaceWidth / 2 - 14, y + 20, 18, 40);
        ctx.fill();
        ctx.stroke();

        if (compressed) {
            ctx.shadowColor = '#ff6b35';
            ctx.shadowBlur = 15;
            ctx.fillStyle = '#ffaa44';
            ctx.beginPath();
            ctx.ellipse(x - furnaceWidth / 2 - 5, y + 40, 3, 10, 0, 0, Math.PI * 2);
            ctx.fill();
            ctx.shadowBlur = 0;
        }

        ctx.fillStyle = '#3a3028';
        ctx.strokeStyle = '#2a2018';
        ctx.beginPath();
        ctx.moveTo(x - 25, y - furnaceHeight / 2);
        ctx.lineTo(x - 15, y - furnaceHeight / 2 - 40);
        ctx.lineTo(x + 15, y - furnaceHeight / 2 - 40);
        ctx.lineTo(x + 25, y - furnaceHeight / 2);
        ctx.closePath();
        ctx.fill();
        ctx.stroke();

        if (this.animationTime % 0.5 < 0.1) {
            ctx.globalAlpha = 0.6;
            ctx.fillStyle = '#555';
            ctx.beginPath();
            ctx.arc(x + (Math.random() - 0.5) * 20, y - furnaceHeight / 2 - 50 - Math.random() * 30, 8 + Math.random() * 6, 0, Math.PI * 2);
            ctx.fill();
            ctx.globalAlpha = 1;
        }
    }

    _drawAirFlow(startX, y, endX, endY) {
        const ctx = this.ctx;
        const count = 12;
        const time = this.animationTime * 3;

        for (let i = 0; i < count; i++) {
            const t = ((i / count) + time) % 1;
            const x = startX + (endX - startX) * t;
            const yOffset = Math.sin(t * Math.PI * 4 + i) * 8;

            ctx.globalAlpha = (1 - Math.abs(t - 0.5) * 1.8) * 0.7;
            const flowGrad = ctx.createLinearGradient(x - 10, y, x + 10, y);
            flowGrad.addColorStop(0, 'rgba(255, 180, 100, 0)');
            flowGrad.addColorStop(0.5, 'rgba(255, 200, 140, 0.8)');
            flowGrad.addColorStop(1, 'rgba(255, 180, 100, 0)');
            ctx.fillStyle = flowGrad;

            ctx.beginPath();
            ctx.ellipse(x, y + yOffset, 14, 5, 0, 0, Math.PI * 2);
            ctx.fill();

            ctx.globalAlpha *= 0.6;
            ctx.fillStyle = 'rgba(255, 220, 160, 0.9)';
            ctx.beginPath();
            ctx.arc(x + 12, y + yOffset, 3, 0, Math.PI * 2);
            ctx.fill();
        }
        ctx.globalAlpha = 1;
    }

    _drawAirFlowReverse(startX, y, endX, endY) {
        const ctx = this.ctx;
        const count = 10;
        const time = this.animationTime * 2.5;

        for (let i = 0; i < count; i++) {
            const t = ((i / count) + time) % 1;
            const x = startX - (startX - endX) * t;
            const yOffset = Math.sin(t * Math.PI * 3 + i) * 5;

            ctx.globalAlpha = (1 - t) * 0.5;
            ctx.strokeStyle = '#4ecdc4';
            ctx.lineWidth = 1.5;
            ctx.beginPath();
            ctx.moveTo(x, y + yOffset - 6);
            ctx.quadraticCurveTo(x - 15, y + yOffset, x, y + yOffset + 6);
            ctx.stroke();
        }
        ctx.globalAlpha = 1;
    }

    _updateSparks(spawnX, spawnY, dt) {
        if (this.phase % (Math.PI / 8) < 0.05 && Math.random() < 0.3) {
            const spark = this.sparkParticles.find(s => s.life <= 0);
            if (spark) {
                spark.x = spawnX;
                spark.y = spawnY + (Math.random() - 0.5) * 20;
                spark.vx = -Math.random() * 80 - 20;
                spark.vy = (Math.random() - 0.5) * 60;
                spark.life = 1;
                spark.maxLife = 0.5 + Math.random() * 0.5;
                spark.size = 2 + Math.random() * 3;
            }
        }

        this.sparkParticles.forEach(s => {
            if (s.life > 0) {
                s.life -= dt / s.maxLife;
                s.x += s.vx * dt;
                s.y += s.vy * dt;
                s.vx *= 0.96;
                s.vy *= 0.96;
            }
        });
    }

    _drawSparks(originX, originY) {
        const ctx = this.ctx;
        this.sparkParticles.forEach(s => {
            if (s.life > 0) {
                ctx.globalAlpha = s.life;
                ctx.shadowColor = '#ff6b35';
                ctx.shadowBlur = 10;
                const grad = ctx.createRadialGradient(s.x, s.y, 0, s.x, s.y, s.size);
                grad.addColorStop(0, '#ffffff');
                grad.addColorStop(0.3, '#ffcc66');
                grad.addColorStop(1, 'rgba(255, 107, 53, 0)');
                ctx.fillStyle = grad;
                ctx.beginPath();
                ctx.arc(s.x, s.y, s.size, 0, Math.PI * 2);
                ctx.fill();
            }
        });
        ctx.globalAlpha = 1;
        ctx.shadowBlur = 0;
    }

    _drawInfoPanel(W, H, dt) {
        const ctx = this.ctx;
        const panelX = W * 0.02;
        const panelY = H * 0.04;
        const panelW = 160;
        const panelH = 110;

        ctx.fillStyle = 'rgba(15, 25, 40, 0.85)';
        ctx.strokeStyle = 'rgba(78, 205, 196, 0.3)';
        ctx.lineWidth = 1;
        this._roundRect(ctx, panelX, panelY, panelW, panelH, 8);
        ctx.fill();
        ctx.stroke();

        ctx.fillStyle = '#9bb0c7';
        ctx.font = '11px sans-serif';
        ctx.textAlign = 'left';
        ctx.textBaseline = 'top';

        const items = [
            { label: '推拉频率', value: `${this.frequency.toFixed(1)} 次/分`, color: '#a78bfa' },
            { label: '风箱行程', value: `${this.stroke.toFixed(1)} cm`, color: '#4a9eff' },
            { label: '风压', value: `${this.windPressure.toFixed(0)} Pa`, color: '#4ecdc4' },
            { label: '风量', value: `${this.airVolume.toFixed(3)} m³/s`, color: '#ffc857' },
        ];

        items.forEach((item, idx) => {
            const iy = panelY + 14 + idx * 23;
            ctx.fillStyle = '#6b7c93';
            ctx.fillText(item.label, panelX + 10, iy);
            ctx.fillStyle = item.color;
            ctx.font = 'bold 12px Consolas, monospace';
            ctx.fillText(item.value, panelX + 10, iy + 13);
            ctx.font = '11px sans-serif';
        });
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

    _tempToHex(temp) {
        const t = Math.max(0, Math.min(1, (temp - 400) / (1600 - 400)));
        let r, g, b;
        if (t < 0.25) {
            const p = t / 0.25;
            r = Math.floor(78 + p * 300);
            g = Math.floor(205 - p * 50);
            b = Math.floor(196 - p * 150);
        } else if (t < 0.5) {
            const p = (t - 0.25) / 0.25;
            r = 255;
            g = Math.floor(200 - p * 50);
            b = Math.floor(87 - p * 87);
        } else {
            const p = (t - 0.5) / 0.5;
            r = 255;
            g = Math.floor(200 - p * 100);
            b = 0;
        }
        return `rgb(${Math.min(255, r)}, ${Math.max(0, Math.min(255, g))}, ${Math.max(0, b)})`;
    }

    setTempData(avgTemp) {
        this.tempData = avgTemp;
    }
}

export default BellowsAnimation;
