import * as THREE from 'three';
import { OrbitControls } from 'three/addons/controls/OrbitControls.js';

export class Furnace3D {
    constructor(containerId) {
        this.container = document.getElementById(containerId);
        if (!this.container) {
            throw new Error(`Container ${containerId} not found`);
        }

        this.furnaceType = 'HAN';
        this.tempData = {
            avg: 1000,
            zones: [850, 920, 970, 1010, 1040]
        };
        this.tempMin = 400;
        this.tempMax = 1600;
        this.animationTime = 0;

        this.init();
    }

    init() {
        const rect = this.container.getBoundingClientRect();
        const width = rect.width || 600;
        const height = rect.height || 400;

        this.scene = new THREE.Scene();
        this.scene.background = new THREE.Color(0x0a0f14);
        this.scene.fog = new THREE.FogExp2(0x0a0f14, 0.015);

        this.camera = new THREE.PerspectiveCamera(45, width / height, 0.1, 1000);
        this.camera.position.set(6, 5, 8);
        this.camera.lookAt(0, 2, 0);

        this.renderer = new THREE.WebGLRenderer({
            antialias: true,
            alpha: true
        });
        this.renderer.setSize(width, height);
        this.renderer.setPixelRatio(Math.min(window.devicePixelRatio, 2));
        this.renderer.shadowMap.enabled = true;
        this.renderer.shadowMap.type = THREE.PCFSoftShadowMap;
        this.renderer.toneMapping = THREE.ACESFilmicToneMapping;
        this.renderer.toneMappingExposure = 1.2;
        this.container.appendChild(this.renderer.domElement);

        this.controls = new OrbitControls(this.camera, this.renderer.domElement);
        this.controls.enableDamping = true;
        this.controls.dampingFactor = 0.05;
        this.controls.minDistance = 4;
        this.controls.maxDistance = 20;
        this.controls.maxPolarAngle = Math.PI / 2 - 0.05;
        this.controls.target.set(0, 2, 0);

        this.setupLights();
        this.buildFurnace();
        this.buildEnvironment();

        this._animate = this._animate.bind(this);
        this._onResize = this._onResize.bind(this);

        window.addEventListener('resize', this._onResize);
        this._animate();
    }

    setupLights() {
        const ambient = new THREE.AmbientLight(0x404060, 0.4);
        this.scene.add(ambient);

        const dirLight = new THREE.DirectionalLight(0xfff0dd, 0.6);
        dirLight.position.set(5, 10, 5);
        dirLight.castShadow = true;
        dirLight.shadow.mapSize.width = 1024;
        dirLight.shadow.mapSize.height = 1024;
        dirLight.shadow.camera.near = 0.5;
        dirLight.shadow.camera.far = 50;
        dirLight.shadow.camera.left = -10;
        dirLight.shadow.camera.right = 10;
        dirLight.shadow.camera.top = 10;
        dirLight.shadow.camera.bottom = -10;
        this.scene.add(dirLight);

        this.fireLight = new THREE.PointLight(0xff6b35, 2.0, 15, 2);
        this.fireLight.position.set(0, 1.5, 0);
        this.fireLight.castShadow = true;
        this.scene.add(this.fireLight);

        this.innerGlow = new THREE.PointLight(0xffaa44, 1.0, 10, 1.5);
        this.innerGlow.position.set(0, 3, 0);
        this.scene.add(this.innerGlow);
    }

    buildEnvironment() {
        const groundGeo = new THREE.PlaneGeometry(30, 30);
        const groundMat = new THREE.MeshStandardMaterial({
            color: 0x2a2520,
            roughness: 0.9,
            metalness: 0.1
        });
        const ground = new THREE.Mesh(groundGeo, groundMat);
        ground.rotation.x = -Math.PI / 2;
        ground.receiveShadow = true;
        this.scene.add(ground);

        const bricks = [];
        const brickMat = new THREE.MeshStandardMaterial({
            color: 0x6b4a3a,
            roughness: 0.85,
            metalness: 0.05
        });
        for (let i = 0; i < 20; i++) {
            const brick = new THREE.Mesh(
                new THREE.BoxGeometry(0.8 + Math.random() * 0.4, 0.3, 0.5),
                brickMat
            );
            brick.position.set(
                (Math.random() - 0.5) * 8,
                0.15,
                -2 - Math.random() * 3
            );
            brick.rotation.y = Math.random() * Math.PI;
            brick.castShadow = true;
            brick.receiveShadow = true;
            bricks.push(brick);
        }
        this.scene.add(...bricks);
    }

    buildFurnace() {
        this.furnaceGroup = new THREE.Group();
        this.tempMeshes = [];
        this.tempMaterials = [];

        const config = this._getFurnaceConfig();

        const baseMat = new THREE.MeshStandardMaterial({
            color: 0x5a4a3a,
            roughness: 0.8,
            metalness: 0.1
        });

        const baseGeo = new THREE.CylinderGeometry(
            config.radius + 0.3,
            config.radius + 0.5,
            0.4,
            32
        );
        const base = new THREE.Mesh(baseGeo, baseMat);
        base.position.y = 0.2;
        base.castShadow = true;
        base.receiveShadow = true;
        this.furnaceGroup.add(base);

        const wallMat = new THREE.MeshStandardMaterial({
            color: 0x8b6914,
            roughness: 0.75,
            metalness: 0.15
        });

        const wallGeo = new THREE.CylinderGeometry(
            config.radius + config.wallThickness,
            config.radius + config.wallThickness + 0.1,
            config.height,
            32,
            1,
            true
        );
        const walls = new THREE.Mesh(wallGeo, wallMat);
        walls.position.y = config.height / 2 + 0.4;
        walls.castShadow = true;
        walls.receiveShadow = true;
        this.furnaceGroup.add(walls);

        const capMat = new THREE.MeshStandardMaterial({
            color: 0x4a3a2a,
            roughness: 0.9,
            metalness: 0.1
        });
        const capGeo = new THREE.CylinderGeometry(
            config.radius + config.wallThickness + 0.15,
            config.radius + config.wallThickness,
            0.3,
            32
        );
        const cap = new THREE.Mesh(capGeo, capMat);
        cap.position.y = config.height + 0.4 + 0.15;
        cap.castShadow = true;
        this.furnaceGroup.add(cap);

        const chimneyMat = new THREE.MeshStandardMaterial({
            color: 0x3a3028,
            roughness: 0.85,
            metalness: 0.1
        });
        const chimneyGeo = new THREE.CylinderGeometry(
            config.radius * 0.3,
            config.radius * 0.35,
            1.2,
            16
        );
        const chimney = new THREE.Mesh(chimneyGeo, chimneyMat);
        chimney.position.y = config.height + 0.4 + 0.3 + 0.6;
        chimney.castShadow = true;
        this.furnaceGroup.add(chimney);

        this._buildTempZones(config);
        this._buildDoor(config);
        this._buildBellows(config);
        this._buildSmoke();
        this._buildIronPool(config);

        this.scene.add(this.furnaceGroup);
    }

    _buildTempZones(config) {
        const zoneHeights = [
            { bottom: 0.4, top: config.height * 0.2, color: 0xff6b35 },
            { bottom: config.height * 0.2, top: config.height * 0.4, color: 0xff8c5a },
            { bottom: config.height * 0.4, top: config.height * 0.65, color: 0xffc857 },
            { bottom: config.height * 0.65, top: config.height * 0.85, color: 0xf0a850 },
            { bottom: config.height * 0.85, top: config.height, color: 0xe09050 }
        ];

        zoneHeights.forEach((zone, idx) => {
            const zoneHeight = zone.top - zone.bottom;
            const zoneRadius = config.radius * 0.95;

            const tempMat = new THREE.MeshBasicMaterial({
                color: new THREE.Color().setHSL(0.05 + idx * 0.03, 0.9, 0.5),
                transparent: true,
                opacity: 0.75,
                side: THREE.DoubleSide
            });

            const tempGeo = new THREE.CylinderGeometry(
                zoneRadius,
                zoneRadius,
                zoneHeight,
                32,
                1,
                true
            );
            const tempMesh = new THREE.Mesh(tempGeo, tempMat);
            tempMesh.position.y = zone.bottom + zoneHeight / 2;

            this.tempMeshes.push(tempMesh);
            this.tempMaterials.push(tempMat);
            this.furnaceGroup.add(tempMesh);

            const innerGeo = new THREE.CylinderGeometry(
                zoneRadius * 0.85,
                zoneRadius * 0.85,
                zoneHeight * 0.9,
                32,
                1,
                true
            );
            const innerMat = new THREE.MeshBasicMaterial({
                color: 0xffffff,
                transparent: true,
                opacity: 0.1,
                side: THREE.BackSide,
                blending: THREE.AdditiveBlending
            });
            const innerMesh = new THREE.Mesh(innerGeo, innerMat);
            innerMesh.position.y = zone.bottom + zoneHeight / 2;
            this.furnaceGroup.add(innerMesh);
        });
    }

    _buildDoor(config) {
        const doorWidth = config.radius * 0.5;
        const doorHeight = 0.9;
        const doorDepth = config.wallThickness + 0.05;

        const doorMat = new THREE.MeshStandardMaterial({
            color: 0x3a2515,
            roughness: 0.9,
            metalness: 0.2
        });
        const doorGeo = new THREE.BoxGeometry(doorWidth, doorHeight, doorDepth);
        const door = new THREE.Mesh(doorGeo, doorMat);
        door.position.set(0, doorHeight / 2 + 0.5, config.radius + config.wallThickness / 2);
        door.castShadow = true;
        this.furnaceGroup.add(door);

        const frameMat = new THREE.MeshStandardMaterial({
            color: 0x6b5030,
            roughness: 0.7,
            metalness: 0.3
        });
        const frameWidth = doorWidth + 0.2;
        const frameHeight = doorHeight + 0.2;
        const frameThickness = 0.1;
        const top = new THREE.Mesh(
            new THREE.BoxGeometry(frameWidth, frameThickness, doorDepth + 0.1),
            frameMat
        );
        top.position.set(0, doorHeight + 0.5 + 0.1, config.radius + config.wallThickness / 2);
        this.furnaceGroup.add(top);
    }

    _buildBellows(config) {
        this.bellowsGroup = new THREE.Group();
        const bx = -config.radius - 1.8;
        const by = 1.2;

        const nozzleMat = new THREE.MeshStandardMaterial({
            color: 0x5a4030,
            roughness: 0.6,
            metalness: 0.3
        });
        const nozzleGeo = new THREE.CylinderGeometry(0.15, 0.25, 0.6, 12);
        const nozzle = new THREE.Mesh(nozzleGeo, nozzleMat);
        nozzle.rotation.z = Math.PI / 2;
        nozzle.position.set(bx + 0.3, by, 0);
        nozzle.castShadow = true;
        this.bellowsGroup.add(nozzle);

        const bodyMat = new THREE.MeshStandardMaterial({
            color: 0x8b6914,
            roughness: 0.7,
            metalness: 0.2
        });
        this.bellowsBody = new THREE.Mesh(
            new THREE.BoxGeometry(1.0, 0.6, 0.6),
            bodyMat
        );
        this.bellowsBody.position.set(bx - 0.5, by, 0);
        this.bellowsBody.castShadow = true;
        this.bellowsGroup.add(this.bellowsBody);

        const handleMat = new THREE.MeshStandardMaterial({
            color: 0x4a3020,
            roughness: 0.8,
            metalness: 0.1
        });
        this.handle = new THREE.Mesh(
            new THREE.CylinderGeometry(0.06, 0.06, 0.9, 12),
            handleMat
        );
        this.handle.rotation.z = Math.PI / 2;
        this.handle.position.set(bx - 1.3, by, 0);
        this.handle.castShadow = true;
        this.bellowsGroup.add(this.handle);

        const supportMat = new THREE.MeshStandardMaterial({
            color: 0x4a3a2a,
            roughness: 0.9,
            metalness: 0.1
        });
        const support1 = new THREE.Mesh(
            new THREE.BoxGeometry(0.1, 1.2, 0.1),
            supportMat
        );
        support1.position.set(bx - 0.3, 0.6, 0.25);
        this.bellowsGroup.add(support1);

        const support2 = support1.clone();
        support2.position.z = -0.25;
        this.bellowsGroup.add(support2);

        this.furnaceGroup.add(this.bellowsGroup);
    }

    _buildSmoke() {
        this.smokeParticles = [];
        const smokeMat = new THREE.MeshBasicMaterial({
            color: 0x444444,
            transparent: true,
            opacity: 0.3,
            depthWrite: false,
            blending: THREE.NormalBlending
        });

        for (let i = 0; i < 30; i++) {
            const smoke = new THREE.Mesh(
                new THREE.SphereGeometry(0.15 + Math.random() * 0.2, 8, 8),
                smokeMat.clone()
            );
            smoke.position.set(
                (Math.random() - 0.5) * 0.3,
                Math.random() * 2,
                (Math.random() - 0.5) * 0.3
            );
            smoke.userData = {
                speed: 0.3 + Math.random() * 0.5,
                drift: (Math.random() - 0.5) * 0.3,
                startY: Math.random() * 2,
                scale: 0.5 + Math.random() * 0.5,
                opacityOffset: Math.random() * Math.PI * 2
            };
            this.smokeParticles.push(smoke);
            this.furnaceGroup.add(smoke);
        }
    }

    _buildIronPool(config) {
        const poolMat = new THREE.MeshStandardMaterial({
            color: 0xff4400,
            emissive: 0xff2200,
            emissiveIntensity: 0.8,
            roughness: 0.3,
            metalness: 0.9
        });
        const poolGeo = new THREE.CylinderGeometry(
            config.radius * 0.7,
            config.radius * 0.75,
            0.15,
            32
        );
        this.ironPool = new THREE.Mesh(poolGeo, poolMat);
        this.ironPool.position.y = 0.5;
        this.furnaceGroup.add(this.ironPool);
    }

    _getFurnaceConfig() {
        if (this.furnaceType === 'MING') {
            return {
                height: 6.5,
                radius: 1.6,
                wallThickness: 0.25
            };
        }
        return {
            height: 4.0,
            radius: 1.1,
            wallThickness: 0.18
        };
    }

    updateTemp(avgTemp, zones) {
        this.tempData.avg = avgTemp;
        if (zones && zones.length >= 5) {
            this.tempData.zones = [...zones];
        }

        const intensity = Math.max(1.5, Math.min(4.0, (avgTemp - 400) / 300));
        this.fireLight.intensity = intensity;
        this.fireLight.color = this._tempToColor(avgTemp);

        this.innerGlow.intensity = intensity * 0.6;
        this.innerGlow.color = this._tempToColor(avgTemp * 0.9);

        this.tempMaterials.forEach((mat, idx) => {
            const temp = this.tempData.zones[idx] || avgTemp;
            const color = this._tempToColor(temp);
            mat.color = color;

            const normalized = (temp - this.tempMin) / (this.tempMax - this.tempMin);
            mat.opacity = 0.5 + normalized * 0.45;
        });

        if (this.ironPool) {
            const hearthTemp = this.tempData.zones[4] || avgTemp;
            const color = this._tempToColor(Math.max(hearthTemp, 1000));
            this.ironPool.material.color = color;
            this.ironPool.material.emissive = color;
            this.ironPool.material.emissiveIntensity = Math.min(1.2, hearthTemp / 1000);
        }
    }

    updateBellows(frequency, stroke) {
        this.bellowsFrequency = frequency || 30;
        this.bellowsStroke = stroke || 40;
    }

    setFurnaceType(type) {
        this.furnaceType = type;
        this.scene.remove(this.furnaceGroup);
        this.buildFurnace();
        this.updateTemp(this.tempData.avg, this.tempData.zones);
    }

    _tempToColor(temp) {
        const t = Math.max(0, Math.min(1, (temp - this.tempMin) / (this.tempMax - this.tempMin)));
        const color = new THREE.Color();

        if (t < 0.25) {
            const p = t / 0.25;
            color.r = 0.3 + p * 0.5;
            color.g = 0.2 + p * 0.4;
            color.b = 0.8 - p * 0.5;
        } else if (t < 0.5) {
            const p = (t - 0.25) / 0.25;
            color.r = 0.8 + p * 0.1;
            color.g = 0.6 + p * 0.3;
            color.b = 0.3 - p * 0.2;
        } else if (t < 0.75) {
            const p = (t - 0.5) / 0.25;
            color.r = 0.9 + p * 0.1;
            color.g = 0.9 - p * 0.2;
            color.b = 0.1 + p * 0.2;
        } else {
            const p = (t - 0.75) / 0.25;
            color.r = 1.0;
            color.g = 0.7 - p * 0.2;
            color.b = 0.3 - p * 0.2;
        }

        return color;
    }

    _animate() {
        requestAnimationFrame(this._animate);
        this.animationTime += 0.016;

        if (this.bellowsBody && this.handle) {
            const freq = (this.bellowsFrequency || 30) / 60;
            const strokeFactor = ((this.bellowsStroke || 40) / 40);
            const phase = Math.sin(this.animationTime * Math.PI * 2 * freq);
            const offset = phase * 0.3 * strokeFactor;

            this.bellowsBody.position.x = -this._getFurnaceConfig().radius - 1.8 - 0.5 - offset * 0.5;
            this.handle.position.x = -this._getFurnaceConfig().radius - 1.8 - 1.3 - offset;

            this.bellowsBody.scale.x = 1.0 + phase * 0.25;
        }

        this.smokeParticles.forEach((smoke) => {
            const ud = smoke.userData;
            smoke.position.y += ud.speed * 0.02;
            smoke.position.x += Math.sin(this.animationTime * 2 + ud.opacityOffset) * 0.005 * ud.drift;
            smoke.position.z += Math.cos(this.animationTime * 2 + ud.opacityOffset) * 0.005 * ud.drift;

            const progress = (smoke.position.y - ud.startY) / 2;
            smoke.material.opacity = Math.max(0, 0.35 - progress * 0.35) *
                (0.7 + 0.3 * Math.sin(this.animationTime * 3 + ud.opacityOffset));
            smoke.scale.setScalar(ud.scale * (1 + progress * 0.8));

            if (smoke.position.y > 10) {
                smoke.position.y = this._getFurnaceConfig().height + 1;
                smoke.position.x = (Math.random() - 0.5) * 0.3;
                smoke.position.z = (Math.random() - 0.5) * 0.3;
            }
        });

        this.controls.update();
        this.renderer.render(this.scene, this.camera);
    }

    _onResize() {
        const rect = this.container.getBoundingClientRect();
        const width = rect.width;
        const height = rect.height;

        this.camera.aspect = width / height;
        this.camera.updateProjectionMatrix();

        this.renderer.setSize(width, height);
    }

    dispose() {
        window.removeEventListener('resize', this._onResize);
        this.renderer.dispose();
    }
}

export default Furnace3D;
