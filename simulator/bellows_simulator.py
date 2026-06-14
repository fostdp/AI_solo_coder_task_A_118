#!/usr/bin/env python3
# -*- coding: utf-8 -*-
"""
古代风箱鼓风冶铁传感器模拟器
模拟Modbus RTU协议传感器，每10秒上报一次数据
支持汉代炒钢炉(HAN-001)和明代高炉(MING-001)
"""

import argparse
import json
import math
import random
import struct
import sys
import time
from dataclasses import dataclass, field
from typing import Dict, List, Optional

import requests

BACKEND_URL = "http://127.0.0.1:8080/api/sensor/report"

MODBUS_SLAVE_ADDR = {
    "HAN-001": 0x01,
    "MING-001": 0x02,
}

REGISTER_MAP = {
    "push_pull_frequency": {"addr": 0x0000, "scale": 100.0, "unit": "次/分"},
    "stroke_length": {"addr": 0x0002, "scale": 10.0, "unit": "cm"},
    "wind_pressure": {"addr": 0x0004, "scale": 10.0, "unit": "Pa"},
    "air_volume": {"addr": 0x0006, "scale": 1000.0, "unit": "m3/s"},
    "furnace_temp": {"addr": 0x0008, "scale": 10.0, "unit": "°C"},
    "co_concentration": {"addr": 0x000A, "scale": 10000.0, "unit": "%"},
    "o2_concentration": {"addr": 0x000C, "scale": 10000.0, "unit": "%"},
    "iron_feed_rate": {"addr": 0x000E, "scale": 100.0, "unit": "kg/s"},
    "coal_feed_rate": {"addr": 0x0010, "scale": 100.0, "unit": "kg/s"},
    "pig_iron_output": {"addr": 0x0012, "scale": 10.0, "unit": "kg"},
    "temp_zone_top": {"addr": 0x0014, "scale": 10.0, "unit": "°C"},
    "temp_zone_upper": {"addr": 0x0016, "scale": 10.0, "unit": "°C"},
    "temp_zone_middle": {"addr": 0x0018, "scale": 10.0, "unit": "°C"},
    "temp_zone_lower": {"addr": 0x001A, "scale": 10.0, "unit": "°C"},
    "temp_zone_hearth": {"addr": 0x001C, "scale": 10.0, "unit": "°C"},
    "reaction_rate": {"addr": 0x001E, "scale": 1000.0, "unit": "mol/s"},
    "energy_efficiency": {"addr": 0x0020, "scale": 1000.0, "unit": "%"},
}


@dataclass
class FurnaceConfig:
    furnace_id: str
    furnace_name: str
    furnace_type: str
    volume_m3: float
    base_frequency: float
    base_stroke: float
    base_pressure: float
    temp_target_min: float
    temp_target_max: float
    temp_initial: float


FURNACE_CONFIGS: Dict[str, FurnaceConfig] = {
    "HAN-001": FurnaceConfig(
        furnace_id="HAN-001",
        furnace_name="汉代炒钢炉一号",
        furnace_type="Han_Chaogang",
        volume_m3=2.5,
        base_frequency=25.0,
        base_stroke=35.0,
        base_pressure=800.0,
        temp_target_min=1200.0,
        temp_target_max=1350.0,
        temp_initial=800.0,
    ),
    "MING-001": FurnaceConfig(
        furnace_id="MING-001",
        furnace_name="明代高炉一号",
        furnace_type="Ming_Blast",
        volume_m3=8.0,
        base_frequency=32.0,
        base_stroke=50.0,
        base_pressure=1500.0,
        temp_target_min=1350.0,
        temp_target_max=1500.0,
        temp_initial=900.0,
    ),
}


@dataclass
class FurnaceState:
    config: FurnaceConfig
    frequency: float = 0.0
    stroke: float = 0.0
    pressure: float = 0.0
    air_volume: float = 0.0
    temp: float = 0.0
    co_conc: float = 0.0
    o2_conc: float = 21.0
    iron_feed: float = 0.0
    coal_feed: float = 0.0
    pig_iron_total: float = 0.0
    temp_zones: Dict[str, float] = field(default_factory=dict)
    reaction_rate: float = 0.0
    energy_efficiency: float = 0.0
    elapsed_seconds: float = 0.0
    phase: str = "heating"
    noise_seed: float = 0.0

    def __post_init__(self):
        self.frequency = self.config.base_frequency
        self.stroke = self.config.base_stroke
        self.pressure = self.config.base_pressure
        self.temp = self.config.temp_initial
        self.iron_feed = 0.5 if self.config.furnace_type == "Han_Chaogang" else 2.0
        self.coal_feed = 0.3 if self.config.furnace_type == "Han_Chaogang" else 1.2
        self.temp_zones = {
            "top": self.temp - 150,
            "upper": self.temp - 80,
            "middle": self.temp - 30,
            "lower": self.temp + 10,
            "hearth": self.temp + 40,
        }
        self.noise_seed = random.random() * 1000


class ModbusRtuSimulator:
    """模拟Modbus RTU协议帧"""

    @staticmethod
    def calc_crc(data: bytes) -> int:
        crc = 0xFFFF
        for byte in data:
            crc ^= byte
            for _ in range(8):
                if crc & 0x0001:
                    crc = (crc >> 1) ^ 0xA001
                else:
                    crc >>= 1
        return crc

    @staticmethod
    def build_read_holding_registers(slave_addr: int, start_addr: int, count: int) -> bytes:
        frame = struct.pack(">BBHH", slave_addr, 0x03, start_addr, count)
        crc = ModbusRtuSimulator.calc_crc(frame)
        return frame + struct.pack("<H", crc)

    @staticmethod
    def build_register_values(values: List[float], scales: List[float]) -> bytes:
        registers = []
        for v, s in zip(values, scales):
            scaled = int(v * s)
            high = (scaled >> 16) & 0xFFFF
            low = scaled & 0xFFFF
            registers.append(high)
            registers.append(low)
        return struct.pack(">" + "H" * len(registers), *registers)


class ThermodynamicsSimulator:
    """简化的热力学模拟引擎"""

    R = 8.314
    MOLAR_MASS_FE2O3 = 159.69
    MOLAR_MASS_C = 12.011
    MOLAR_MASS_FE = 55.845
    HEAT_REACTION_FE2O3 = -824000.0
    SPECIFIC_HEAT_IRON = 650.0
    SPECIFIC_HEAT_AIR = 1005.0
    AIR_DENSITY = 1.225

    @staticmethod
    def arrhenius_rate(
        temp: float,
        activation_energy: float,
        pre_exp_factor: float,
        o2_conc: float,
        coal_feed: float,
    ) -> float:
        temp_k = temp + 273.15
        rate = pre_exp_factor * math.exp(-activation_energy / (ThermodynamicsSimulator.R * temp_k))
        rate *= max(0.1, o2_conc / 21.0)
        rate *= max(0.01, coal_feed)
        return rate

    @staticmethod
    def heat_generated(
        reaction_rate: float,
        coal_feed: float,
        time_step: float,
    ) -> float:
        heat_reaction = -ThermodynamicsSimulator.HEAT_REACTION_FE2O3 * reaction_rate * time_step
        heat_combustion = coal_feed * time_step * 32000000.0 * 0.8
        return heat_reaction + heat_combustion

    @staticmethod
    def heat_lost(
        temp: float,
        ambient_temp: float,
        volume: float,
        time_step: float,
        heat_loss_coeff: float = 0.015,
    ) -> float:
        surface_area = 6 * (volume ** (2.0 / 3.0))
        delta_t = max(0, temp - ambient_temp)
        return heat_loss_coeff * surface_area * delta_t * time_step

    @staticmethod
    def air_heat_carry(
        air_volume: float,
        air_preheat_temp: float,
        inlet_temp: float,
        time_step: float,
    ) -> float:
        mass_air = air_volume * time_step * ThermodynamicsSimulator.AIR_DENSITY
        return mass_air * ThermodynamicsSimulator.SPECIFIC_HEAT_AIR * (air_preheat_temp - inlet_temp)

    @staticmethod
    def calc_iron_output(reaction_rate: float, time_step: float) -> float:
        fe_mol_per_reaction = 2.0
        iron_mass_rate = reaction_rate * fe_mol_per_reaction * ThermodynamicsSimulator.MOLAR_MASS_FE
        return iron_mass_rate * time_step / 1000.0


class BellowsSimulator:
    """风箱模拟器 - 模拟物理风箱的鼓风特性"""

    @staticmethod
    def calc_air_volume(frequency: float, stroke: float, bore_area: float) -> float:
        cycles_per_sec = frequency / 60.0
        stroke_m = stroke / 100.0
        volume_per_cycle = bore_area * stroke_m * 2
        return cycles_per_sec * volume_per_cycle * 0.75

    @staticmethod
    def calc_pressure(frequency: float, stroke: float, resistance: float) -> float:
        velocity = (stroke / 100.0) * (frequency / 60.0) * 2
        return resistance * velocity * velocity * 0.5 * 1.225 * 1000


def noise(seed: float, t: float, scale: float) -> float:
    return scale * (math.sin(seed + t * 0.5) * 0.3 + random.uniform(-0.5, 0.5))


def simulate_step(state: FurnaceState, dt: float = 10.0,
                  fixed_frequency: Optional[float] = None,
                  fixed_stroke: Optional[float] = None,
                  freq_noise: float = 3.0,
                  stroke_noise: float = 2.5) -> Dict:
    state.elapsed_seconds += dt
    t = state.elapsed_seconds

    activation_e = 160000.0 if state.config.furnace_type == "Han_Chaogang" else 165000.0
    pre_exp = 5.0e8 if state.config.furnace_type == "Han_Chaogang" else 6.5e8
    heat_loss_coeff = 0.015 if state.config.furnace_type == "Han_Chaogang" else 0.012
    air_preheat = 200.0 if state.config.furnace_type == "Han_Chaogang" else 300.0
    bore_area = 0.08 if state.config.furnace_type == "Han_Chaogang" else 0.15

    if fixed_frequency is not None:
        state.frequency = fixed_frequency + noise(state.noise_seed, t, freq_noise)
        state.config.base_frequency = fixed_frequency
    else:
        state.frequency = state.config.base_frequency + noise(state.noise_seed, t, freq_noise)

    if fixed_stroke is not None:
        state.stroke = fixed_stroke + noise(state.noise_seed + 1, t, stroke_noise)
        state.config.base_stroke = fixed_stroke
    else:
        state.stroke = state.config.base_stroke + noise(state.noise_seed + 1, t, stroke_noise)

    state.air_volume = BellowsSimulator.calc_air_volume(state.frequency, state.stroke, bore_area)
    state.pressure = BellowsSimulator.calc_pressure(state.frequency, state.stroke, 3.5)
    state.pressure += noise(state.noise_seed + 2, t, 50.0)

    if state.phase == "heating":
        if state.temp >= state.config.temp_target_min - 50:
            state.phase = "operating"
            state.iron_feed = 0.8 if state.config.furnace_type == "Han_Chaogang" else 2.5
            state.coal_feed = 0.4 if state.config.furnace_type == "Han_Chaogang" else 1.5
    elif state.phase == "operating":
        if random.random() < 0.02:
            state.phase = "disturbance"
            state.coal_feed *= 0.7
    elif state.phase == "disturbance":
        if random.random() < 0.1:
            state.phase = "operating"
            state.coal_feed = 0.4 if state.config.furnace_type == "Han_Chaogang" else 1.5

    state.reaction_rate = ThermodynamicsSimulator.arrhenius_rate(
        state.temp, activation_e, pre_exp, state.o2_conc, state.coal_feed
    )

    heat_in = ThermodynamicsSimulator.heat_generated(state.reaction_rate, state.coal_feed, dt)
    heat_in += ThermodynamicsSimulator.air_heat_carry(state.air_volume, air_preheat, 25.0, dt)
    heat_out = ThermodynamicsSimulator.heat_lost(
        state.temp, 25.0, state.config.volume_m3, dt, heat_loss_coeff
    )

    mass_load = state.config.volume_m3 * 3500.0
    delta_temp = (heat_in - heat_out) / (mass_load * ThermodynamicsSimulator.SPECIFIC_HEAT_IRON)
    state.temp += delta_temp
    state.temp += noise(state.noise_seed + 3, t, 5.0)
    state.temp = max(25.0, min(state.config.temp_target_max + 200, state.temp))

    o2_consumed = min(state.o2_conc * 0.5, state.reaction_rate * dt * 32.0 / 1000.0)
    state.o2_conc = max(2.0, 21.0 - o2_consumed + (state.air_volume * dt * 0.005))
    state.o2_conc = min(21.0, state.o2_conc)

    stoich_ratio = 0.6
    actual_ratio = state.coal_feed / max(0.001, state.iron_feed)
    if actual_ratio > stoich_ratio:
        state.co_conc = min(8.0, (actual_ratio - stoich_ratio) * 5 + state.reaction_rate * 0.001)
    else:
        state.co_conc = max(0.0, state.reaction_rate * 0.0005)
    state.co_conc += noise(state.noise_seed + 4, t, 0.1)

    iron_this_step = ThermodynamicsSimulator.calc_iron_output(state.reaction_rate, dt)
    if state.phase == "operating":
        state.pig_iron_total += iron_this_step * (state.iron_feed / 2.0)
    else:
        state.pig_iron_total += iron_this_step * 0.1

    gradient = (state.temp - (state.temp - 200)) / 5.0
    state.temp_zones["hearth"] = state.temp + 30 + noise(state.noise_seed + 5, t, 3)
    state.temp_zones["lower"] = state.temp + 10 + noise(state.noise_seed + 6, t, 3)
    state.temp_zones["middle"] = state.temp - 20 + noise(state.noise_seed + 7, t, 3)
    state.temp_zones["upper"] = state.temp - 70 + noise(state.noise_seed + 8, t, 3)
    state.temp_zones["top"] = state.temp - 140 + noise(state.noise_seed + 9, t, 3)

    for k in state.temp_zones:
        state.temp_zones[k] = max(25.0, state.temp_zones[k])

    max_heat = state.coal_feed * dt * 32000000.0
    if max_heat > 0:
        effective_heat = heat_in - heat_out
        state.energy_efficiency = max(0.0, min(95.0, (effective_heat / max_heat) * 100.0))
    else:
        state.energy_efficiency = 0.0

    modbus_raw = {}
    for key, info in REGISTER_MAP.items():
        if key in ("pig_iron_output",):
            val = state.pig_iron_total
        elif key.startswith("temp_zone_"):
            zone = key.replace("temp_zone_", "")
            val = state.temp_zones.get(zone, state.temp)
        else:
            attr_map = {
                "push_pull_frequency": "frequency",
                "stroke_length": "stroke",
                "wind_pressure": "pressure",
                "air_volume": "air_volume",
                "furnace_temp": "temp",
                "co_concentration": "co_conc",
                "o2_concentration": "o2_conc",
                "iron_feed_rate": "iron_feed",
                "coal_feed_rate": "coal_feed",
                "reaction_rate": "reaction_rate",
                "energy_efficiency": "energy_efficiency",
            }
            val = getattr(state, attr_map.get(key, "temp"), state.temp)
        modbus_raw[key] = val

    modbus_frame = ModbusRtuSimulator.build_read_holding_registers(
        MODBUS_SLAVE_ADDR.get(state.config.furnace_id, 1),
        0x0000,
        0x0022,
    )

    return {
        "furnace_id": state.config.furnace_id,
        "furnace_type": state.config.furnace_type,
        "push_pull_frequency": round(state.frequency, 3),
        "stroke_length": round(state.stroke, 3),
        "wind_pressure": round(state.pressure, 3),
        "air_volume": round(state.air_volume, 5),
        "furnace_temp": round(state.temp, 2),
        "co_concentration": round(state.co_conc, 4),
        "o2_concentration": round(state.o2_conc, 4),
        "iron_feed_rate": round(state.iron_feed, 4),
        "coal_feed_rate": round(state.coal_feed, 4),
        "pig_iron_output": round(state.pig_iron_total, 3),
        "temp_zone_top": round(state.temp_zones["top"], 2),
        "temp_zone_upper": round(state.temp_zones["upper"], 2),
        "temp_zone_middle": round(state.temp_zones["middle"], 2),
        "temp_zone_lower": round(state.temp_zones["lower"], 2),
        "temp_zone_hearth": round(state.temp_zones["hearth"], 2),
        "reaction_rate": round(state.reaction_rate, 5),
        "energy_efficiency": round(state.energy_efficiency, 2),
        "quality": round(98.0 + random.uniform(-2, 0), 1),
        "protocol": "Modbus_RTU",
        "phase": state.phase,
        "modbus_frame_hex": modbus_frame.hex(),
        "modbus_registers": modbus_raw,
    }


def send_to_backend(data: Dict) -> bool:
    try:
        headers = {"Content-Type": "application/json"}
        response = requests.post(BACKEND_URL, json=data, headers=headers, timeout=5)
        if response.status_code == 200:
            result = response.json()
            if result.get("success"):
                action = result.get("recommended_action", {})
                return True, action
        return False, {}
    except requests.exceptions.RequestException as e:
        return False, {}


def apply_rl_action(state: FurnaceState, action: Dict):
    if "frequency" in action:
        state.frequency = max(10.0, min(60.0, action["frequency"]))
        state.config.base_frequency = state.frequency
    if "stroke" in action:
        state.stroke = max(15.0, min(80.0, action["stroke"]))
        state.config.base_stroke = state.stroke


def main():
    parser = argparse.ArgumentParser(description="风箱鼓风冶铁传感器模拟器")
    parser.add_argument(
        "--furnaces",
        nargs="+",
        default=["HAN-001", "MING-001"],
        help="模拟的冶炼炉ID列表",
    )
    parser.add_argument(
        "--interval",
        type=int,
        default=10,
        help="上报间隔（秒）",
    )
    parser.add_argument(
        "--backend",
        type=str,
        default=BACKEND_URL,
        help="后端上报接口地址",
    )
    parser.add_argument(
        "--verbose",
        "-v",
        action="store_true",
        help="详细输出模式",
    )
    parser.add_argument(
        "--dry-run",
        action="store_true",
        help="只模拟不上报",
    )
    parser.add_argument(
        "--freq",
        type=float,
        default=None,
        help="固定风箱推拉频率 (次/分)，默认使用内置动态模型",
    )
    parser.add_argument(
        "--stroke",
        type=float,
        default=None,
        help="固定风箱行程 (cm)，默认使用内置动态模型",
    )
    parser.add_argument(
        "--freq-noise",
        type=float,
        default=3.0,
        help="频率高斯噪声强度 (默认3.0)",
    )
    parser.add_argument(
        "--stroke-noise",
        type=float,
        default=2.5,
        help="行程高斯噪声强度 (默认2.5)",
    )
    parser.add_argument(
        "--follow-rl",
        action="store_true",
        default=True,
        help="遵循后端RL返回的动作覆盖freq/stroke（默认开启）",
    )
    parser.add_argument(
        "--no-follow-rl",
        action="store_false",
        dest="follow_rl",
        help="忽略后端RL返回的动作，坚持使用--freq/--stroke",
    )
    args = parser.parse_args()

    global BACKEND_URL
    BACKEND_URL = args.backend

    states = {}
    for fid in args.furnaces:
        if fid in FURNACE_CONFIGS:
            states[fid] = FurnaceState(config=FURNACE_CONFIGS[fid])
            print(f"[初始化] {FURNACE_CONFIGS[fid].furnace_name} ({fid}) - "
                  f"目标温度: {FURNACE_CONFIGS[fid].temp_target_min}-"
                  f"{FURNACE_CONFIGS[fid].temp_target_max}°C")

    if not states:
        print("错误: 没有有效的冶炼炉配置")
        sys.exit(1)

    print(f"\n[模拟器启动] 共 {len(states)} 座炉, 上报间隔 {args.interval}s")
    if args.freq is not None:
        print(f"[固定参数] 频率={args.freq} 次/分, 噪声=±{args.freq_noise}")
    if args.stroke is not None:
        print(f"[固定参数] 行程={args.stroke} cm, 噪声=±{args.stroke_noise}")
    print(f"[后端地址] {BACKEND_URL}")
    print(f"[RL跟随] {'开启' if args.follow_rl else '关闭'}")
    print(f"[模式] {'DRY RUN' if args.dry_run else '正常上报'}")
    print("-" * 80)

    step_count = 0
    try:
        while True:
            step_count += 1
            ts = time.strftime("%Y-%m-%d %H:%M:%S")

            for fid, state in states.items():
                data = simulate_step(
                    state,
                    dt=args.interval,
                    fixed_frequency=args.freq,
                    fixed_stroke=args.stroke,
                    freq_noise=args.freq_noise,
                    stroke_noise=args.stroke_noise,
                )

                if args.verbose or step_count % 6 == 0:
                    print(f"[{ts}] [{fid}] "
                          f"炉温:{data['furnace_temp']:7.1f}°C | "
                          f"频率:{data['push_pull_frequency']:5.1f}/min | "
                          f"行程:{data['stroke_length']:4.1f}cm | "
                          f"风压:{data['wind_pressure']:6.0f}Pa | "
                          f"风量:{data['air_volume']:6.3f}m³/s | "
                          f"CO:{data['co_concentration']:5.3f}% | "
                          f"生铁:{data['pig_iron_output']:8.1f}kg | "
                          f"效率:{data['energy_efficiency']:5.1f}% | "
                          f"阶段:{data['phase']}")

                if not args.dry_run:
                    success, action = send_to_backend(data)
                    if success and action and args.follow_rl:
                        apply_rl_action(state, action)
                        if args.verbose:
                            print(f"          ↳ RL动作: 频率→{action.get('frequency', 'N/A')}, "
                                  f"行程→{action.get('stroke', 'N/A')}")

            time.sleep(args.interval)

    except KeyboardInterrupt:
        print("\n\n[模拟器停止] 用户中断")
        for fid, state in states.items():
            print(f"  [{fid}] 生铁累计产量: {state.pig_iron_total:.1f} kg, "
                  f"运行时长: {state.elapsed_seconds/60:.1f} 分钟")


if __name__ == "__main__":
    main()
