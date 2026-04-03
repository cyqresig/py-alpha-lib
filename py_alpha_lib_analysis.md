# py-alpha-lib 深度分析报告

> 分析时间: 2026-04-03
> 源码仓库: [py-alpha-lib](https://github.com/msd-rs/py-alpha-lib)
> 对比项目: [quant-ml-qlib](file:///Users/chenyiqin/work/self-business/quant-ml-qlib)
> 现有引擎: [indicators-computation](file:///Users/chenyiqin/work/self-business/back-end_python-service_ml_pipeline/common/rust/indicators-computation)

---

## 目录

- [第一部分：因子复现能力评估](#第一部分因子复现能力评估)
- [第二部分：完全替代现有 Rust 引擎的可行性分析](#第二部分完全替代现有-rust-引擎的可行性分析)
- [第三部分：Fork py-alpha-lib 支持 WASM 的源码改造分析](#第三部分fork-py-alpha-lib-支持-wasm-的源码改造分析)
- [第四部分：最终结论与推荐策略](#第四部分最终结论与推荐策略)

---

# 第一部分：因子复现能力评估

## 1. 当前架构现状

### 1.1 关键发现：ML 管线并未直接依赖 Qlib

经过对 `ml-pipeline` 代码库的全面搜索，**没有找到任何直接 import 或引用 qlib/Alpha158/alphalib 的代码**。
Qlib 表达式引擎仅存在于独立项目 `quant-ml-qlib` 中，定位为**研究工具 / 因子试验场**，不参与生产管线。

```
┌──────────────────────────────────┐      ┌──────────────────────────────┐
│   当前生产管线 (ml-pipeline)      │      │  研究辅助 (quant-ml-qlib)     │
│                                  │      │                              │
│  ClickHouse → Rust 指标引擎      │      │  ClickHouse → Qlib 表达式引擎 │
│            → ML 训练/预测        │      │  (Alpha158/AlphaTalib/360)   │
│            → Go API → Vue 前端   │      │           → 因子研究/快速试验  │
└──────────────────────────────────┘      └──────────────────────────────┘
         ▲                                          │
         └── 验证有效后手动移植 ─────────────────────┘
```

### 1.2 quant-ml-qlib 中的因子集全景

| 因子集 | 定义位置 | 因子数 | 核心原理 |
|--------|---------|--------|----------|
| **Alpha360** | `Alpha360DL` (loader.py:366) | 360 | 过去 60 天 OHLCV+VWAP 归一化时序展开 |
| **Alpha158** | `Alpha158DL` (loader.py:423) | ~158 | KBar(9) + Price(4) + Rolling(~145)，滚动统计特征 |
| **AlphaTalib** | `AlphaTalibDL` (loader.py:4) | ~300+ | TA-Lib 全量技术指标集成 |
| **AlphaZoo** | `AlphaZoo` (handler.py:160) | ~6 | 实验性 POC（RSI/SMA/T3/PPO + CDL模式） |

## 2. py-alpha-lib 能力剖析

### 2.1 项目概览

| 维度 | 详情 |
|------|------|
| **语言** | Rust 50.7% + Python 49.3% |
| **接口** | PyO3 Python 绑定 (`pip install py-alpha-lib`) |
| **版本** | v0.2.0 (2026-02-25)，活跃维护 |
| **Star** | 88 |
| **协议** | BSD-2-Clause |
| **性能** | Alpha101 全量(101因子 × 4000只 × 261天) = **3.9秒**，比 pandas 快 **729x** |

### 2.2 支持的算子完整列表 (46 个)

| 类别 | 算子 | 说明 |
|------|------|------|
| **滚动均线** | MA, EMA, DMA, SMA, LWMA | 简单/指数/自定义权重/线性加权移动平均 |
| **统计** | STDDEV, VAR, CORR, CORR2, COV, KURTOSIS, SKEWNESS, ZSCORE | 标准差/方差/相关/协方差/峰度/偏度/Z分 |
| **线性回归** | SLOPE, INTERCEPT, REGBETA, REGRESI | 斜率/截距/回归系数/回归残差 |
| **极值** | HHV, HHVBARS, LLV, LLVBARS | 最高/最低值及距今天数 |
| **累计/窗口** | SUM, SUMIF, SUMBARS, PRODUCT, COUNT, COUNT_NANS | 求和/条件求和/累计天数/乘积/计数 |
| **排名** | RANK, CC_RANK, GROUP_RANK, BINS | 时序排名/截面排名/分组排名/分箱 |
| **引用** | REF | 历史引用（前 N 天） |
| **条件** | BACKFILL, BARSLAST, BARSSINCE | 前向填充/距上次条件为真天数 |
| **交叉** | CROSS, RCROSS, LONGCROSS, RLONGCROSS | 金叉/死叉/连续金叉/连续死叉 |
| **累计特殊** | SCAN_ADD, SCAN_MUL | 条件累加/累乘 |
| **其他** | ENTROPY, MOMENT, NEUTRALIZE, WEIGHTED_DELAY, FRET, CC_ZSCORE, GROUP_ZSCORE, MIN_MAX_DIFF | 熵/中心矩/中性化/加权延迟/未来收益率等 |

### 2.3 内置因子集

| 因子集 | 数量 | 状态 | 性能 |
|--------|------|------|------|
| **WorldQuant Alpha 101** | 101/101 | ✅ 完整实现 | 4000只×261天 = **3.9秒** |
| **GTJA 国泰君安 Alpha 191** | 190/191 | ✅ 几乎完整 | 4000只×261天 = **~4.5秒** |

### 2.4 Qlib 不具备的独特能力

- **CROSS/RCROSS/LONGCROSS**: 金叉/死叉检测 — 对事件驱动管线天然适配
- **BARSLAST/BARSSINCE**: 条件距今天数 — 构建时序特征极有价值
- **SCAN_ADD/SCAN_MUL**: 累计条件加/乘 — 自引用 Alpha 表达式
- **NEUTRALIZE**: 行业中性化 — 截面因子处理
- **ENTROPY**: 滚动信息熵 — 市场微观结构指标
- **FRET**: 未来收益率 — 内置标签生成

## 3. 逐因子集复现能力评估

### 3.1 Alpha360 — ✅ 100% 可复现

Alpha360 的完整逻辑极其简单：

```python
# Alpha360 = 过去 60 天 × 6 列 (close/open/high/low/vwap/volume)
# 每列归一化后展开为 60 个特征，共 360 个

for i in range(59, 0, -1):
    fields += ["Ref($close, %d)/$close" % i]  # CLOSE59 ~ CLOSE1
fields += ["$close/$close"]                     # CLOSE0 = 1.0
# 同理 open, high, low, vwap, volume
```

**py-alpha-lib 复现（仅需 `REF` 算子 + numpy 除法）：**

```python
import alpha
import numpy as np

features = {}
for i in range(59, 0, -1):
    features[f"CLOSE{i}"] = alpha.REF(close, i) / close
    features[f"OPEN{i}"]  = alpha.REF(open_, i) / close
    features[f"HIGH{i}"]  = alpha.REF(high, i)  / close
    features[f"LOW{i}"]   = alpha.REF(low, i)   / close
    features[f"VWAP{i}"]  = alpha.REF(vwap, i)  / close
    features[f"VOL{i}"]   = alpha.REF(volume, i) / (volume + 1e-12)
# 当天值
features["CLOSE0"] = np.ones_like(close)
features["OPEN0"]  = open_ / close
# ... 共 360 个
```

### 3.2 Alpha158 — ✅ ~95% 可复现

| Alpha158 因子类别 | 因子数 | py-alpha-lib 覆盖 | 说明 |
|-------------------|--------|-------------------|------|
| **KBar (K线形态)** | 9 | ✅ numpy 组合 | `(close-open)/open` 等式 |
| **Price (价格)** | 4 | ✅ `REF` | `REF(close, d) / close` |
| **ROC (动量)** | 5 | ✅ `REF` | `REF(close, d) / close` |
| **MA (均线)** | 5 | ✅ `MA` | `MA(close, d) / close` |
| **STD (波动率)** | 5 | ✅ `STDDEV` | `STDDEV(close, d) / close` |
| **BETA (斜率)** | 5 | ✅ `SLOPE` | `SLOPE(close, d) / close` |
| **RSQR (R²)** | 5 | ⚠️ 可推导 | 用 `SLOPE` + `REGRESI` + `STDDEV` 推导 |
| **RESI (残差)** | 5 | ✅ `REGRESI` | 直接对应 |
| **MAX/MIN** | 10 | ✅ `HHV/LLV` | 直接对应 |
| **QTLU/QTLD** | 10 | ⚠️ numpy 补充 | 无原生 Quantile，用 numpy 一行搞定 |
| **RANK** | 5 | ✅ `RANK` | 直接对应 |
| **RSV** | 5 | ✅ 组合 | `(close - LLV(low,d)) / (HHV(high,d) - LLV(low,d))` |
| **IMAX/IMIN** | 10 | ✅ `HHVBARS/LLVBARS` | 直接对应 |
| **IMXD** | 5 | ✅ 组合 | `(HHVBARS - LLVBARS) / d` |
| **CORR/CORD** | 10 | ✅ `CORR2` | 直接对应 |
| **CNTP/CNTN/CNTD** | 15 | ✅ `COUNT` + 条件 | 组合实现 |
| **SUMP/SUMN/SUMD** | 15 | ✅ `SUMIF` | 条件求和 |
| **VMA/VSTD** | 10 | ✅ `MA/STDDEV` | 对 volume 列操作 |
| **WVMA** | 5 | ⚠️ 组合 | 复杂组合表达式 |
| **VSUMP/VSUMN/VSUMD** | 15 | ✅ `SUMIF` | 类似 SUMP 系列 |

#### Quantile 算子说明

**Quantile（滚动分位数）**：在过去 N 天的价格窗口里，找出"排在第 X% 位置"的那个值。

```python
# 滚动 Quantile 用 numpy 实现（几行代码）
from numpy.lib.stride_tricks import sliding_window_view
windows = sliding_window_view(close, 20)
qtlu = np.quantile(windows, 0.8, axis=1) / close[19:]
```

#### Rsquare 算子说明

**Rsquare（滚动 R²）**：价格对时间做线性回归的拟合优度（0~1）。

```python
resi = alpha.REGRESI(close, time_idx, 20)
r_squared = 1.0 - alpha.VAR(resi, 20) / alpha.VAR(close, 20)
```

### 3.3 AlphaTalib — ✅ ~85% 可组合复现

**核心发现：大多数经典技术指标本质上就是滚动窗口计算的组合，py-alpha-lib 的原子算子完全可以组合实现。**

#### RSI（相对强弱指数）— ✅ 可组合

```python
delta = close - alpha.REF(close, 1)
gain = np.maximum(delta, 0)
loss = np.maximum(-delta, 0)
avg_gain = alpha.EMA(gain, 14)
avg_loss = alpha.EMA(loss, 14)
rsi = 100 - 100 / (1 + avg_gain / avg_loss)
```

#### MACD — ✅ 可组合

```python
macd_line = alpha.EMA(close, 12) - alpha.EMA(close, 26)
signal    = alpha.EMA(macd_line, 9)
histogram = macd_line - signal
```

#### BBANDS（布林带）— ✅ 可组合

```python
middle = alpha.MA(close, 20)
std    = alpha.STDDEV(close, 20)
upper  = middle + 2 * std
lower  = middle - 2 * std
```

#### ATR（平均真实波幅）— ✅ 可组合

```python
prev_close = alpha.REF(close, 1)
tr = np.maximum(high - low, np.maximum(
    np.abs(high - prev_close), np.abs(low - prev_close)
))
atr = alpha.MA(tr, 14)
```

#### 真正无法组合的部分 ❌

| 类型 | 因子数 | 原因 | 对 ML 模型的影响 |
|------|--------|------|-----------------|
| **CDL 蜡烛图形态** (CDL_HAMMER, CDL_DOJI 等) | ~60 | 不是数学公式，是**规则型模式识别** | 🟢 **影响极小** — 树模型/神经网络更擅长从原始 OHLC 自行学习 |
| **HT_* 希尔伯特变换** | ~5 | 信号处理算法，需 DSP 库 | 🟢 **极少使用** |

### 3.4 WorldQuant Alpha 101 — ✅ 100% 已内置

```bash
python -m examples.wq101.main --with-al -s 1 -e 102
# 101 因子 × 4000只 × 261天 = 3.9 秒
```

### 3.5 GTJA 191 — ✅ 99.5% 已内置

```bash
python -m examples.gtja191.al           # 全部跑
python -m examples.gtja191.al 143       # 跑单个
```

## 4. 性能对比

| 实现方式 | 计算规模 (4000只 × 261天) | 性能量级 |
|----------|--------------------------|---------| 
| **pandas** (Python) | ~2675 秒 (44分钟) | 1x |
| **TA-Lib** (C) | 快于 pandas，单线程 | ~10-50x |
| **polars_ta** | ~58 秒 | 46x |
| **py-alpha-lib** (Rust + rayon 并行) | **~3.9 秒** | **729x** 🚀 |

---

# 第二部分：完全替代现有 Rust 引擎的可行性分析

> 目标: 评估 `py-alpha-lib` 作为新一代 Rust 计算基座，**完全替代**现有 `indicators-computation` 引擎 + Qlib 因子计算
>
> 约束: 必须同时满足 **PyO3 → Python** 和 **WASM → Web 浏览器** 双编译目标

## 5. 现有 Rust 引擎能力全景图

现有引擎 (`common/rust/indicators-computation`) 共包含 **8 个层级**，远不仅仅是"指标计算"：

### 5.1 完整能力层级

| 层级 | 模块 | 功能 | 函数数 | 消费端 |
|------|------|------|--------|--------|
| **L1** | `indicators/` | 19 个自研技术指标 | 19 | PyO3 + WASM |
| **L1b** | `indicators/slope`, `pullback` | 派生指标 (slope_pct/ols/atr, pullback) | 4 | PyO3 + WASM |
| **L2** | `batch.rs` | JSON 驱动的多指标批量计算 | 1 | PyO3 + WASM |
| **L3** | `rti/` | rust_ti 桥接的 100+ RTI 指标批量 | 1 | PyO3 + WASM |
| **L4** | `signal_resonance/` | 15 种信号判定 + 综合评分 + 背离多策略 | 2 | PyO3 + WASM |
| **L5** | `compute_all.rs` | All-in-One 全量计算 | 1 | PyO3 + WASM |
| **L6** | `grid/` | 单指标多参数网格 (rayon 并行) | 1 | PyO3 |
| **L7** | `parallel/` | 多 Symbol rayon 并行计算 | 1 | PyO3 |
| **L8** | `ml_features/` | ML 管线 47 维特征批量 | 1 | PyO3 |
| **P** | `patterns/` | Pivot / ZigZag / CUSUM / Divergence / Crossover | 7+ | PyO3 + WASM |

### 5.2 详细指标清单 (L1: 19 个自研指标)

| 指标 | 文件 | 输出形式 |
|------|------|----------|
| SMA | sma.rs | Single |
| EMA | ema.rs | Single |
| WMA | wma.rs | Single |
| RSI | rsi.rs | Single |
| MACD | macd.rs | Macd (dif/dea/histogram) |
| Bollinger | bollinger.rs | Bollinger (upper/middle/lower) |
| Boll Deviation | boll_deviation.rs | Single |
| Bollinger %B | bollinger_pct_b.rs | Single |
| BBW | bbw.rs | Single |
| KDJ | kdj.rs | Kdj (k/d/j) |
| OBV | obv.rs | DualLine |
| ARBR | arbr.rs | DualLine |
| VR | vr.rs | DualLine |
| PSY | psy.rs | DualLine |
| OSC | osc.rs | DualLine |
| BIAS | bias.rs | TripleLine |
| MAVOL | mavol.rs | TripleLine |
| Slope (3 种) | slope.rs | Single |
| Pullback | pullback.rs | Bool flags |

### 5.3 信号共振引擎 (L4: 15 种规则)

```
Signal Resonance Engine
├── 阈值类 (7种): RSI/CCI/WilliamsR/Stochastic/CMO/MFI/IBS
├── 交叉类 (2种): MACD金叉死叉 / KDJ金叉死叉
├── 方向类 (3种): Supertrend方向 / Aroon趋势 / ADX强度
├── 位置类 (1种): Bollinger位置判定
└── 背离类 (多策略×多指标): 11种指标 × N 种背离策略
```

### 5.4 模式检测 (P: 7+ 函数)

| 功能 | 算法 | 描述 |
|------|------|------|
| Pivot High/Low | pivothigh/pivotlow | 局部高低点检测 |
| ZigZag | percentage / ATR 双模式 | 曲折拐点检测 |
| CUSUM | 4 种 baseline × 3 种 reset | 累积偏差事件检测 |
| Divergence | 多策略重构版 | 价格-指标背离检测 |
| Crossover | 前后 K 线交叉 | 金叉/死叉判定 |
| Threshold | 超买/超卖阈值 | 阈值突破判定 |
| Direction | 方向性判定 | 趋势方向判定 |

### 5.5 编译目标

| 目标 | 技术 | 消费端 | 依赖 |
|------|------|--------|------|
| **WASM** | `wasm-bindgen` + `wasm-pack` | Web 浏览器图表 | `wasm-bindgen`, `js-sys`, `serde-wasm-bindgen` |
| **PyO3** | `pyo3` + `maturin` | ML Pipeline Python 服务 | `pyo3`, `numpy` |
| **C FFI** | `cdylib` + C header | Go 后端服务 | `libc` |

## 6. 逐层替代可行性评估

### 6.1 L1: 19 个自研指标

| 指标 | py-alpha-lib 覆盖 | 实现方式 | 难度 |
|------|:--:|------|:--:|
| SMA | ✅ | `MA(close, N)` | 🟢 |
| EMA | ✅ | `EMA(close, N)` | 🟢 |
| WMA | ✅ | `WMA(close, N)` | 🟢 |
| RSI | ✅ 组合 | `EMA(gain, N)` / `EMA(loss, N)` + numpy | 🟡 |
| MACD | ✅ 组合 | `EMA(close, 12) - EMA(close, 26)` + `EMA(dif, 9)` | 🟡 |
| Bollinger | ✅ 组合 | `MA(close, 20) ± 2 * STDDEV(close, 20)` | 🟡 |
| Boll Deviation | ✅ 组合 | `(price - lower) / (upper - lower)` | 🟡 |
| Bollinger %B | ✅ 组合 | 同上 | 🟡 |
| BBW | ✅ 组合 | `(upper - lower) / middle` | 🟡 |
| KDJ | ⚠️ 组合 | `RANK_IN_WINDOW(close-min, max-min, 9)` → 需扩展 | 🟠 |
| OBV | ⚠️ 组合 | `SUM_IF(volume, close>prev_close)` → 需 `SCAN_ADD` | 🟠 |
| ARBR | ❌ | 需自定义：`SUM((H-O)/(H-L), N)` | 🔴 |
| VR | ❌ | 需自定义：条件分组量比 | 🔴 |
| PSY | ❌ | 需自定义：`SUM(close>prev_close, N) / N * 100` | 🔴 |
| OSC | ⚠️ 组合 | `(close - MA) / MA * 100` + EMA | 🟡 |
| BIAS | ✅ 组合 | `(close - MA) / MA * 100` | 🟢 |
| MAVOL | ✅ | `MA(volume, N)` 三条 | 🟢 |
| Slope | ✅ 组合 | `(data[i] - data[i-N]) / data[i-N]` → REF | 🟡 |
| Pullback | ❌ | 需自定义：多条件交叉判定 | 🔴 |

**L1 评估: ~60% 可直接覆盖，~25% 需组合，~15% 需扩展/自定义**

### 6.2 L1b-L8 其他层

| 层 | 替代可行性 | 说明 |
|:--:|:--:|------|
| L2 Batch | ⚠️ | py-alpha-lib 无批量 JSON 驱动 API，需自建编排层 |
| L3 RTI | ❌ | py-alpha-lib **不包含** rust_ti 的 100+ 指标 |
| L4 信号共振 | ❌ | 完全独有的业务逻辑层 |
| L5 All-in-One | ⚠️ | 需重建编排层 |
| L6 参数网格 | ❌ | py-alpha-lib 无此概念 |
| L7 多 Symbol | ✅ | py-alpha-lib 的 groups 并行即此场景 |
| L8 ML 47 维 | ⚠️ | 部分因子可替代，但需重建编排 |
| P Patterns | ❌ | Pivot/ZigZag/CUSUM/Divergence 完全独有 |

### 6.3 不可替代的独有能力清单

| 能力 | 描述 | py-alpha-lib 替代方案 |
|------|------|:--:|
| 信号共振引擎 | 15 种规则 + 综合评分 + 快照构建 | ❌ 纯业务逻辑 |
| Pivot/ZigZag | 局部极值 + 曲折拐点检测 | ❌ |
| CUSUM | 4×3 模式累积偏差事件检测 | ❌ |
| Divergence | 多策略×多指标背离检测 | ❌ |
| RTI 桥接 | ADX/Aroon/Supertrend/CCI/SAR/Ichimoku 等 | ❌ |
| ML 47 维编排 | 精心调参的编排层 | ⚠️ 部分 |

---

# 第三部分：Fork py-alpha-lib 支持 WASM 的源码改造分析

## 7. py-alpha-lib 源码架构 × rayon 渗透深度

### 7.1 三层架构与 rayon 着力点

经过对 py-alpha-lib 完整源码的逐文件审计，rayon 的使用呈现**清晰的层级隔离**：

```
┌─────────────────────────────────────────────────────────┐
│  Layer 3: lib.rs (PyO3 绑定层)                           │
│  ┌─────────────────────────────────────────────────────┐ │
│  │  rayon 使用点 ① — List 输入的多 symbol 并行          │ │
│  │                                                     │ │
│  │  r.into_par_iter()                                  │ │
│  │    .zip(input.into_par_iter())                      │ │
│  │    .map(|(mut out, input)| { ... })                 │ │
│  │    .collect_into_vec(&mut _r);                      │ │
│  └─────────────────────────────────────────────────────┘ │
├─────────────────────────────────────────────────────────┤
│  Layer 2: algo/group.rs (分组算子)                        │
│  ┌─────────────────────────────────────────────────────┐ │
│  │  rayon 使用点 ② — group_rank / group_zscore         │ │
│  │                                                     │ │
│  │  (0..group_size).into_par_iter().for_each(|j| {     │ │
│  │      // 每个时间步骤并行处理                          │ │
│  │  });                                                │ │
│  └─────────────────────────────────────────────────────┘ │
├─────────────────────────────────────────────────────────┤
│  Layer 1: algo/ 其他所有算子 (纯计算核心)                  │
│  ┌─────────────────────────────────────────────────────┐ │
│  │  ❌ 完全不依赖 rayon!                                │ │
│  │                                                     │ │
│  │  ma.rs / ema.rs / stddev.rs / rank.rs / sum.rs     │ │
│  │  slope.rs / moments.rs / cross.rs / scan.rs ...    │ │
│  │  → 全部是 fn ta_xxx(ctx, r, input, ...) 签名        │ │
│  │  → 在单个 &mut [f64] 上做顺序计算                    │ │
│  └─────────────────────────────────────────────────────┘ │
└─────────────────────────────────────────────────────────┘
```

**⚠️ 关键发现（已勘误）**: 上方三层架构图为初始假设。经 grep 实测验证，**19 个 algo 核心模块文件**均在函数内部使用了 rayon 的 `par_chunks_mut`/`par_chunks` 进行多 symbol groups 并行（如 `ta_ma` 中 `r.par_chunks_mut().zip(input.par_chunks()).for_each()`）。rayon 渗透到了 Layer 1 的几乎所有算子。但好消息是：使用模式完全统一，且 `for_each` 闭包内部的计算逻辑确实是零线程依赖的纯函数，只需将 `par_chunks` 替换为 `chunks` 即可切换串行模式。

### 7.2 algo/ 目录完整模块列表 (实测: 19/23 个文件使用 rayon)

```
src/algo/
├── mod.rs          ← 公共 re-export，无 rayon
├── backfill.rs     ← ta_backfill
├── context.rs      ← Context (start/end/groups/flags)
├── cross.rs        ← ta_cross / ta_rcross / ta_longcross
├── ema.rs          ← ta_ema / ta_dma / ta_sma / ta_lwma
├── entropy.rs      ← ta_entropy
├── error.rs        ← Error 枚举
├── extremum.rs     ← ta_hhv / ta_llv / ta_hhvbars / ta_llvbars
├── group.rs        ← ta_group_rank / ta_group_zscore  ★ 唯一含 rayon
├── ma.rs           ← ta_ma
├── misc.rs         ← ta_barslast / ta_barssince / ta_count_nans / ...
├── moments.rs      ← ta_moment / ta_kurtosis / ta_skewness
├── neutralize.rs   ← ta_neutralize
├── rank.rs         ← ta_rank / ta_cc_rank / ta_bins
├── returns.rs      ← ta_fret
├── scan.rs         ← ta_scan_add / ta_scan_mul
├── series.rs       ← ta_ref / ta_weighted_delay
├── skip_nan_window.rs  ← 内部辅助
├── slope.rs        ← ta_slope / ta_intercept / ta_regbeta / ta_regresi
├── stats.rs        ← ta_corr / ta_corr2 / ta_cov
├── stddev.rs       ← ta_stddev / ta_var
├── sum.rs          ← ta_sum / ta_sumif / ta_sumbars / ta_product / ta_count
└── zscore.rs       ← ta_zscore / ta_cc_zscore / ta_min_max_diff
```

### 7.3 rayon 具体使用清单 (逐处审计)

| 文件 | 位置 | rayon API | 用途 |
|------|------|-----------|------|
| **algo/group.rs** | `ta_group_rank()` | `(0..N).into_par_iter().for_each()` | 时间步并行排名 |
| **algo/group.rs** | `ta_group_zscore()` | `(0..N).into_par_iter().for_each()` | 时间步并行 Z-Score |
| **lib.rs** | `ema()` 手写模板 | `r.into_par_iter().zip(input.into_par_iter())` | List 输入并行 |
| **build.rs 生成代码** | 每个算子的 List 分支 (×46) | `into_par_iter().zip().map().collect_into_vec()` | List 输入并行 |

> 注意：`build.rs` 是**代码生成器**，会在编译时扫描 `src/algo/*.rs` 所有 `pub fn ta_xxx` 签名，自动生成 PyO3 绑定代码。生成的代码中包含 `into_par_iter()`。

## 8. Feature-Gate 改造方案

### 8.1 目标: 同一份源码 → 多种消费方式并存

```
py-alpha-lib (fork)
    │
    ├── maturin build --features "parallel,python"   → PyO3 版本 (因子研究)
    │   └── rayon 多核并行 + pip install ✅
    │
    └── default-features = false                      → 纯 Rust 库
        └── indicators-computation Cargo 依赖 → PyO3 + WASM ✅
```

### 8.2 Cargo.toml 改造

```toml
[features]
default = []
parallel = ["rayon"]                          # PyO3 编译时启用
python   = ["pyo3", "numpy", "pyo3-log"]      # PyO3 绑定
wasm     = ["wasm-bindgen", "js-sys"]          # WASM 绑定

[dependencies]
# 可选依赖：仅在对应 feature 下引入
rayon         = { version = "1.11", optional = true }
pyo3          = { version = "0.28", features = ["abi3"], optional = true }
numpy         = { version = "0.28", optional = true }
pyo3-log      = { version = "0.13", optional = true }
wasm-bindgen  = { version = "0.2", optional = true }
js-sys        = { version = "0.3", optional = true }

# 始终需要的
num-traits = "0.2"
log = "0.4"
```

### 8.3 algo/group.rs 改造 (2 处)

```rust
#[cfg(feature = "parallel")]
use rayon::prelude::*;

pub fn ta_group_rank<NumT: Float>(...) {
    // ...
    let r_ptr = UnsafePtr::new(r.as_mut_ptr(), r.len());

    #[cfg(feature = "parallel")]
    (0..group_size).into_par_iter().for_each(|j| {
        let r = r_ptr.get();
        // ... 并行逻辑 (使用 UnsafePtr 跨线程)
    });

    #[cfg(not(feature = "parallel"))]
    for j in 0..group_size {
        // ... 串行逻辑 (直接操作 &mut [f64]，更安全)
    }
}
// ta_group_zscore 同理
```

### 8.4 lib.rs 改造 (PyO3 条件编译)

```rust
#[cfg(feature = "python")]
mod algo_impl { ... }  // 现有 PyO3 绑定代码

#[cfg(feature = "python")]
#[pymodule]
fn _algo(m: &Bound<'_, PyModule>) -> PyResult<()> { ... }
```

### 8.5 build.rs 代码生成器改造

生成的 List 分支代码模板中，`into_par_iter()` 改为条件分支：

```rust
// build.rs 生成的代码：
#[cfg(feature = "parallel")]
{
    r.into_par_iter()
        .zip(input.into_par_iter())
        .map(|(mut out, input)| { ... })
        .collect_into_vec(&mut _r);
}
#[cfg(not(feature = "parallel"))]
{
    let _r: Vec<_> = r.into_iter()
        .zip(input.into_iter())
        .map(|(mut out, input)| { ... })
        .collect();
}
```

### 8.6 WASM 绑定层 (已不需要)

> ℹ️ **策略变更**: py-alpha-lib 不再直接导出 WASM，而是作为纯 Rust 库被 `indicators-computation` 依赖。WASM 编译由 `indicators-computation` 负责，输出完整指标函数（如 `wasm.rsi(close, 14)` 一次调用出结果），而非裸露原子算子要求 JS 端手动组合。因此不再需要新建 `src/wasm.rs` 绑定层。

## 9. 改造工作量精确评估

### 9.1 改动量矩阵

| 文件 | 改动类型 | 改动行数 | 风险 | 影响现有逻辑 |
|------|----------|:--------:|:----:|:----:|
| `Cargo.toml` | 依赖改 `optional` + features 声明 | ~15 行 | 🟢 无 | ❌ 不影响 |
| `algo/group.rs` | 2 处 `into_par_iter` 加 `#[cfg]` 分支 | ~10 行 | 🟢 无 | ❌ 不影响 |
| `lib.rs` | 现有 PyO3 绑定加 `#[cfg(feature = "python")]` | ~5 行 | 🟢 无 | ❌ 不影响 |
| `build.rs` | 代码生成模板中 `into_par_iter` → `#[cfg]` | ~6 处 | 🟡 低 | ❌ 不影响 |
| `src/wasm.rs` (新建) | WASM 绑定层 (`#[wasm_bindgen]` 导出 46 算子) | ~230 行 | 🟡 低 | ❌ 新文件 |
| `algo/*.rs` (19 个文件) | `par_chunks` → `chunks` 条件编译（可用宏简化） | ~60-100 行 | 🟡 低 | ❌ 不影响 |

**合计改动: ~400-420 行 (含 ~60-100 行 algo 条件编译修改)**

> ⚠️ 注意：原估计 ~260 行严重低估，因未考虑 19 个 algo 模块的 rayon 渗透。

### 9.2 风险与注意事项

#### ⚠️ 风险 1: `build.rs` 是代码生成器

py-alpha-lib 用 `build.rs` 在编译时**自动扫描 `src/algo/*.rs`**，解析所有 `pub fn ta_xxx` 签名，然后**生成** `algo_bindings.rs`（PyO3 绑定代码）。生成的代码里硬编码了 `into_par_iter()`。

**影响**: 你不是改源码里的 `into_par_iter`，而是要改**代码生成器模板**里的 `into_par_iter`。
**难度**: 🟡 中等 — 需要理解 `build.rs` 的模板逻辑（~800 行），但改动点本身很集中（约 6 处字符串模板）。

#### ⚠️ 风险 2: `UnsafePtr` 在串行模式下多余

`group.rs` 里为了跨线程修改 `&mut [f64]` 而包装了 `UnsafePtr`（含 `unsafe impl Send/Sync`）。串行模式下完全不需要它。

```rust
// group.rs 里的 unsafe 包装
struct UnsafePtr<NumT> { ptr: *mut NumT, len: usize }
unsafe impl<NumT> Send for UnsafePtr<NumT> {}
unsafe impl<NumT> Sync for UnsafePtr<NumT> {}
```

**影响**: 不改也不出错，但串行分支白白多一层 unsafe 包装。
**建议**: 串行分支直接用安全代码，不走 `UnsafePtr`。

#### ⚠️ 风险 3: WASM 绑定层需要新建 (~230 行)

py-alpha-lib 只有 PyO3 绑定，**没有 WASM 绑定层**。你需要新建 `src/wasm.rs`，为 46 个算子写 `#[wasm_bindgen]` 导出。

**影响**: 这是工作量最大的一块（~230 行新代码），但每个函数都是重复模式。
**风险**: 🟢 低 — 每个导出函数只是调用对应的 `ta_xxx`，无复杂逻辑。

#### ⚠️ 风险 4: 上游同步成本

py-alpha-lib 仍在活跃开发（最近提交 2 个月前）。Fork 之后：
- 上游新增算子需手动 merge
- 上游改 `build.rs` 可能与你的 `#[cfg]` 冲突

**降低风险**: 给上游提 PR 合入 rayon feature-gate。改动量小、不影响现有行为，作者大概率接受。

#### ⚠️ 风险 5: 测试矩阵翻倍

改之前只需测 1 种配置，改之后需要 CI 里跑 **2 个编译目标**：

```yaml
# CI 需要同时验证两种编译
- maturin build --features "parallel,python"  # PyO3
- wasm-pack build --features "wasm"            # WASM
```

需确保两个 feature 组合都能编译通过、算子结果一致。

### 9.3 风险矩阵汇总

| 风险点 | 严重程度 | 应对策略 |
|--------|:--------:|---------|
| `build.rs` 代码生成器模板 | 🟡 中 | 修改 ~6 处字符串模板，加 `#[cfg]` 条件 |
| `UnsafePtr` 串行多余 | 🟢 低 | 不改也不出错；洁癖可加 `#[cfg]` 区分 |
| WASM 绑定层需新建 | 🟠 中偏高 | ~230 行新代码，重复模式，工作量最大 |
| 上游同步/merge 冲突 | 🟡 中 | 建议提 PR 合入上游 |
| 测试矩阵翻倍 | 🟢 低 | CI 加一个 job |

---

# 第四部分：最终结论与推荐策略

## 10. 因子复现能力汇总

| 因子集 | 因子数 | py-alpha-lib 复现能力 | 核心依赖算子 |
|--------|--------|----------------------|-------------|
| **Alpha360** | 360 | ✅ **100%** | 仅 `REF` + numpy 除法 |
| **Alpha101** (WQ) | 101 | ✅ **100% 已内置** | 开箱即用 |
| **GTJA 191** | 191 | ✅ **99.5% 已内置** (190/191) | 开箱即用 |
| **Alpha158** | ~158 | ✅ **~95%** | `MA/EMA/STDDEV/SLOPE/CORR2/HHV/LLV/RANK/REF` + numpy |
| **AlphaTalib** | ~300+ | ⚠️ **~85%** | RSI/MACD/BBANDS/ATR 可组合；CDL/HT 无法覆盖 |

## 11. 现有引擎替代可行性汇总

| 维度 | 可替代比例 | 说明 |
|------|:--:|------|
| L1 自研指标 (19) | ~60% | SMA/EMA/WMA/BIAS/MAVOL 等可替代，KDJ/ARBR/VR/PSY 不可 |
| L3 RTI 指标 (100+) | ~0% | ADX/Aroon/Supertrend 等无对标 |
| L4 信号共振 | 0% | 纯业务逻辑，不可替代 |
| Patterns (7+) | ~10% | 仅 CROSS 可替代 |
| Qlib 因子能力 | ~95% | Alpha158/360/101/GTJA191 几乎完全覆盖 |

## 12. Feature-Gate 改造汇总

| 问题 | 答案 |
|------|------|
| rayon 写死了吗？ | ✅ 是的，19 个 algo 核心模块均使用 `par_chunks_mut`/`par_chunks` |
| pyo3/numpy 写死了吗？ | ✅ 是的，均为 `Cargo.toml` 硬依赖 |
| 核心计算逻辑有线程依赖吗？ | ❌ 没有，`for_each` 闭包内部为纯串行逻辑 |
| rayon 使用模式统一吗？ | ✅ 全部是 `par_chunks_mut().zip(par_chunks()).for_each()` |
| 改造是否影响现有逻辑？ | ❌ 所有改动均通过 `#[cfg(feature)]` 隔离，零影响 |
| 最大工作量在哪？ | 19 个 algo 文件的 `par_chunks` → `chunks` 条件编译（可用宏简化） |
| 最大风险在哪？ | `build.rs` 代码生成器模板需理解后修改 |

## 13. 推荐策略："算子下沉，指标上浮"

### 13.1 目标架构

```
py-alpha-lib (fork, feature-gated)
├── features: parallel (rayon), python (pyo3/numpy)
├── src/algo/*  ← 原子算子 (ta_ma, ta_ema, ta_stddev ...)
│   └── 条件编译: parallel → par_chunks / 非 parallel → chunks
└── 两种使用方式:
    ├── pip install (maturin build --features parallel,python)
    │   └── Python 端因子研究 (Alpha158/360/101/GTJA191 组合)
    └── Rust crate 依赖 (default-features = false)
        └── indicators-computation 引入作为底层算子基座

indicators-computation (自有引擎, 保留)
├── Cargo.toml: alpha = { path = "../py-alpha-lib", default-features = false }
├── src/indicators/   ← 完整指标 (rsi/macd/boll/kdj...)
│   └── 内部调用 alpha::algo::ta_ema() 等算子
├── src/signal_resonance/  ← 信号共振引擎 (保留不变)
├── src/patterns/          ← 模式检测 (保留不变)
├── 编译 → PyO3 (给 ML Pipeline)
└── 编译 → WASM (给 Web 浏览器)
```

### 13.2 核心收益

- **算子去重**: SMA/EMA/STDDEV 等基础算子只维护一份 Rust 实现
- **Python 端加速**: 因子研究直接使用 py-alpha-lib 的 rayon 批量并行
- **WASM 不受影响**: 通过 indicators-computation 编译，输出完整指标函数
- **独有能力保留**: 信号共振/模式检测/RTI 桥接等不受影响

### 13.3 实施路线

| 阶段 | 行动 | 工作量 | 优先级 |
|------|------|--------|--------|
| **Phase 1** | py-alpha-lib feature-gate 改造 (rayon + pyo3 optional) | 1-2 天 | P0 |
| **Phase 2** | 按需扩展缺失算子 (ta_quantile 等)，遵循 add_algo skill | 0.5-1 天 | P0 |
| **Phase 3** | indicators-computation 引入 alpha crate，逐步重构指标实现 | 2-3 天 | P1 |
| **Phase 4** | 验证 PyO3 + WASM 双编译目标通过 | 0.5 天 | P1 |

## 14. 集成代码示例

```python
import alpha
import numpy as np
from alpha.context import ExecContext

# 1. 从 ClickHouse 加载 OHLCV
ohlcv = load_from_clickhouse(symbol, start, end)
close = ohlcv["close"].to_numpy().astype(np.float64)
high = ohlcv["high"].to_numpy().astype(np.float64)
low = ohlcv["low"].to_numpy().astype(np.float64)
volume = ohlcv["volume"].to_numpy().astype(np.float64)

# 2. 设置严格周期模式（匹配 pandas rolling 行为）
alpha.set_ctx(flags=alpha.FLAG_STRICTLY_CYCLE)

# ========== Alpha158 等价因子 ==========
features = {
    "KMID": (close - ohlcv["open"].to_numpy()) / ohlcv["open"].to_numpy(),
    "MA5":     alpha.MA(close, 5) / close,
    "MA20":    alpha.MA(close, 20) / close,
    "STD20":   alpha.STDDEV(close, 20) / close,
    "BETA20":  alpha.SLOPE(close, 20) / close,
    "MAX20":   alpha.HHV(high, 20) / close,
    "MIN20":   alpha.LLV(low, 20) / close,
    "CORR20":  alpha.CORR2(close, np.log(volume + 1), 20),
}

# ========== 经典指标 (组合实现) ==========
delta = close - alpha.REF(close, 1)
gain = np.maximum(delta, 0)
loss = np.maximum(-delta, 0)
features["RSI14"] = 100 - 100 / (1 + alpha.EMA(gain, 14) / (alpha.EMA(loss, 14) + 1e-12))

macd_line = alpha.EMA(close, 12) - alpha.EMA(close, 26)
signal = alpha.EMA(macd_line, 9)
features["MACD"] = macd_line
features["MACD_HIST"] = macd_line - signal

# ========== py-alpha-lib 独有 (事件驱动适配) ==========
features["BARS_SINCE_LOW"] = alpha.BARSLAST(close <= alpha.LLV(close, 60))
features["GOLDEN_CROSS_5_20"] = alpha.CROSS(alpha.MA(close, 5), alpha.MA(close, 20))
features["ENTROPY20"] = alpha.ENTROPY(close, 20)
```
