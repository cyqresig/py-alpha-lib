# 技术深度解析：ddof / EMA 初始化 / ta_ref / RSI 平滑 / RTI 替换分析

---

## 1. StdDev 的 ddof=0 vs ddof=1

### 含义

| 名称 | 公式 | 除数 | 术语 |
|------|------|------|------|
| **ddof=0** (Population) | `σ² = Σ(xᵢ - x̄)² / N` | N | 总体标准差 |
| **ddof=1** (Sample) | `s² = Σ(xᵢ - x̄)² / (N-1)` | N-1 | 样本标准差 |

### 具体差异（举例）

数据 `[1, 2, 3]`，N=3，均值=2：
```
Population (ddof=0): σ = sqrt((1+0+1)/3)  = sqrt(0.667) = 0.8165
Sample    (ddof=1): s = sqrt((1+0+1)/2)  = sqrt(1.000) = 1.0000
```

**N=20 时差异缩小**：`ddof=0` 除以 20，`ddof=1` 除以 19，差异仅 ~5%。金融时序中 N 通常≥10，实际影响很小。

### 各平台/库的选择

| 平台/库 | 默认 ddof | 备注 |
|---------|----------|------|
| **Pandas** `rolling().std()` | **1** (Sample) | 官方文档明确标注 |
| **NumPy** `np.std()` | **0** (Population) | 但 `ddof=1` 可选 |
| **TA-Lib** Bollinger | **0** (Population) | 行业标准 |
| **TradingView** | **0** (Population) | 行业标准 |
| **Bloomberg** | **0** (Population) | 行业标准 |
| 你的当前代码 | **0** (Population) | `bollinger.rs` L62: `variance_sum / period_usize` |
| alpha `ta_stddev` | **1** (Sample) | `stddev.rs` L85: `var / (count - 1)` |

### 🏆 最佳实践

> **金融技术分析领域**：**ddof=0 (Population)** 是行业标准。

原因：
1. Bollinger Bands 发明者 John Bollinger 的原始论文使用 Population 标准差
2. TA-Lib、TradingView、Bloomberg 等行业标准工具均使用 ddof=0
3. 滚动窗口本身就是一个完整的"总体"（窗口内的全部数据），而非从更大总体中抽样

### ✅ 建议

**保持 ddof=0 不变**。为 `alpha::ta_stddev` 增加一个 `ddof` 参数（或新增 `ta_stddev_pop` 函数），使其支持两种模式。这样既可以保持金融计算的行业标准，又不影响 alpha 库在其他场景的使用。

---

## 2. EMA 初始化：SMA 种子 vs 首值种子

### 两种方式的代码对比

**你的当前代码** (`indicators/ema.rs`)：
```rust
// 初始值 = 前 period 个数据的 SMA
let sma = compute_sma(data, period)?;
let initial_ema = sma[period_usize - 1];       // ← SMA seed
result[period_usize - 1] = initial_ema;          // ← 从 index=period-1 开始
// 前 period-1 个位置为 NaN
```

**alpha 的 `ta_ema`** (`py-alpha-lib/src/algo/ema.rs`)：
```rust
let mut prev = i[0];         // ← 首值作为种子 (input[0])
for (n, (r, c)) in ... {
    *r = weight * *c + k * prev; // ← 从 index=0 就开始输出
    prev = *r;
}
```

### 直观理解

假设数据 `[10, 11, 12, 13, 14]`，period=3，α=2/(3+1)=0.5：

**SMA 种子**（你的代码）：
```
idx 0: NaN
idx 1: NaN
idx 2: SMA(10,11,12) = 11.0         ← 种子
idx 3: 0.5 × 13 + 0.5 × 11 = 12.0
idx 4: 0.5 × 14 + 0.5 × 12 = 13.0
```

**首值种子**（alpha）：
```
idx 0: 0.5 × 10 + 0.5 × 10 = 10.0  ← 首值种子
idx 1: 0.5 × 11 + 0.5 × 10 = 10.5
idx 2: 0.5 × 12 + 0.5 × 10.5 = 11.25
idx 3: 0.5 × 13 + 0.5 × 11.25 = 12.125
idx 4: 0.5 × 14 + 0.5 × 12.125 = 13.0625
```

**可以看到**：两种方式在前几个值上差异显著（index 2: 11.0 vs 11.25），但随着序列增长，差异会**指数级衰减**（EMA 的"遗忘"特性），50-100 根 K线后基本收敛。

### 各平台/库的选择

| 平台/库 | 初始化方式 | 输出起始 |
|---------|-----------|---------|
| **Pandas** `ewm().mean()` | 首值种子 (首个非NaN) | index 0 |
| **TA-Lib** | **SMA 种子** | index = period-1 |
| **TradingView** | **SMA 种子** | index = period-1 |
| **Bloomberg** | **SMA 种子** | index = period-1 |
| 你的当前代码 | **SMA 种子** | index = period-1 |
| alpha `ta_ema` | 首值种子 | index 0 |

### 🏆 最佳实践

> **金融技术分析领域**：**SMA 种子** 是行业标准。

原因：
1. TA-Lib 和 TradingView（全球使用量最大的两个 TA 平台）都用 SMA 种子
2. SMA 种子不输出前 period-1 个值(NaN)，避免了初始化噪声
3. 首值种子会让早期数据严重偏向第一个数据点

### ✅ 建议

**保持你的 SMA 种子实现不变**，不迁移 EMA 到 alpha。如果未来需要在 alpha 中支持 SMA 种子模式，可以新增 `ta_ema_sma_seeded` 函数。

---

## 3. `ta_ref`（延迟/位移）是什么

### 含义

`ta_ref` 就是把时间序列**向右平移 N 个位置**（也叫 "lag" / "shift"）。

等价于 **Pandas 的 `shift(N)`**。

### 直观示例

```
输入:  [10, 11, 12, 13, 14]
ta_ref(input, periods=2):
输出:  [NaN, NaN, 10, 11, 12]
                 ↑    ↑    ↑
              2天前 的值被搬到了当前位置
```

**金融含义**：`ta_ref(close, 2)` = "2 天前的收盘价"

### 用 ta_ref 计算收益率

```
close       = [100, 102, 101, 105, 103]
prev_close  = ta_ref(close, 1) = [NaN, 100, 102, 101, 105]
return_1    = (close - prev_close) / prev_close
            = [NaN, 0.02, -0.0098, 0.0396, -0.019]
```

### 为何说"收益不大"？

你的手写 `compute_return` 本身就只有一行核心：`(close[i] - close[i-n]) / close[i-n]`，已经足够简洁。用 `ta_ref` 反而需要先生成一个中间数组再做 element-wise 除法，多一次内存分配。

---

## 4. RSI 的 Wilder's Smoothing vs 标准 EMA

### 两种平滑方式的核心区别

**本质是 α（平滑系数）的计算方式不同**：

| 平滑方式 | α 公式 | N=14 时的 α | 等效 EMA 周期 |
|---------|--------|------------|-------------|
| **Wilder's** | `α = 1/N` | 1/14 = **0.0714** | 等效 EMA(27) |
| **标准 EMA** | `α = 2/(N+1)` | 2/15 = **0.1333** | EMA(14) |

### 直观理解

**Wilder (α=0.0714)** → 更慢、更平滑，对新数据反应更迟钝  
**标准 EMA (α=0.1333)** → 更快、对新数据反应更灵敏

两者的递推公式形式相同：`avg = α × new + (1-α) × prev_avg`  
区别仅在于 α 的值。但这个 α 的差异（0.07 vs 0.13，差了近一倍）对 RSI 值的影响是**实质性的**。

### 数值影响举例

假设 N=14，前 14 天都涨了 1 元（avg_gain=1, avg_loss=0），第 15 天跌了 2 元：

**Wilder's (α=1/14)**:
```
avg_gain = (1 × 13 + 0) / 14 = 0.9286
avg_loss = (0 × 13 + 2) / 14 = 0.1429
RS = 0.9286 / 0.1429 = 6.5
RSI = 100 - 100/7.5 = 86.67
```

**标准 EMA (α=2/15)**:
```
avg_gain = 0.1333 × 0 + 0.8667 × 1 = 0.8667
avg_loss = 0.1333 × 2 + 0.8667 × 0 = 0.2667
RS = 0.8667 / 0.2667 = 3.25
RSI = 100 - 100/4.25 = 76.47
```

**差值超过 10 个点 (86.67 vs 76.47)**——完全不可接受的差异。

### 各平台/库的选择

| 平台/库 | RSI 平滑方式 |
|---------|-------------|
| **TA-Lib** | **Wilder's** (α=1/N) |
| **TradingView** | **Wilder's** (α=1/N) |
| **Bloomberg** | **Wilder's** (α=1/N) |
| **东方财富/同花顺** | **Wilder's** (α=1/N) |
| 你的当前代码 | **Wilder's** (α=1/N) ← L84: `(avg_gain * (period - 1) + gains[i]) / period` |
| alpha `ta_ema` | 标准 EMA (α=2/(N+1)) — **不匹配** |

### 🏆 最佳实践

> RSI **必须使用 Wilder's Smoothing**，这是 J. Welles Wilder 在 1978 年原始论文中定义的算法。所有主流平台都遵循此标准。使用标准 EMA 计算的 RSI 不是 "RSI"。

### ✅ 建议

**绝对不要替换 RSI 为 alpha 的 `ta_ema`**。如果未来想统一，应在 alpha 中新增 `ta_wilder_smooth` 算子（α=1/N），而非修改 RSI 的实现。

---

## 5. RTI 委托指标 → alpha-lib 替换分析

RTI 层（`rti/core/*`）目前全部委托给 `rust_ti` 外部 crate。逐个分析：

### 5.1 可用 alpha 替换的 RTI 指标

| RTI 指标 | 当前实现 | alpha 可用算子 | 替换可行性 | 收益 |
|---------|---------|---------------|-----------|------|
| **CCI** | `rust_ti::cci` (SMA + MAD) | `ta_ma` + 手写 MAD | 🟡 **部分可行** | CCI = (TP - SMA(TP)) / (0.015 × MAD)。SMA 可用 `ta_ma`，MAD(平均绝对偏差) alpha 无直接算子，需手写。整体收益不大 |
| **Aroon Up/Down** | `rust_ti::aroon_up/down` | `ta_hhvbars` / `ta_llvbars` | ✅ **直接可行** | Aroon_Up = ((period - bars_since_HHV) / period) × 100。`ta_hhvbars` 直接提供 bars_since_HHV |
| **Aroon Oscillator** | `rust_ti::aroon_indicator` | `ta_hhvbars` + `ta_llvbars` | ✅ **直接可行** | Osc = Aroon_Up - Aroon_Down，完全可用 alpha 算子组合 |
| **Stochastic Oscillator** | `rust_ti::stochastic` | `ta_hhv` + `ta_llv` | ✅ **直接可行** | %K = (close - LLV) / (HHV - LLV) × 100。这与你现有 KDJ RSV 部分的逻辑一致 |
| **Williams %R** | `rust_ti::williams_r` | `ta_hhv` + `ta_llv` | ✅ **直接可行** | %R = (HHV - close) / (HHV - LLV) × -100。与 Stochastic 非常类似 |
| **ROC** | `rust_ti::roc` | `ta_ref` | ✅ **直接可行** | ROC = (close - close[t-1]) / close[t-1]。等价于 `(close - ta_ref(close, 1)) / ta_ref(close, 1)` |

### 5.2 不建议替换的 RTI 指标

| RTI 指标 | 当前实现 | 不替换原因 |
|---------|---------|-----------|
| **MFI** | `rust_ti::mfi` | 需要 positive/negative money flow 分类累计，alpha 无对应。用 `ta_sumif` 理论可行但不如 rust_ti 简洁 |
| **Parabolic SAR** | `rust_ti::parabolic_sar` | 状态机逻辑（趋势翻转、加速因子递增），alpha 完全不覆盖 |
| **Directional Movement (ADX/DI)** | `rust_ti::dms` | 复合多步算法(TR→DM→+DI/-DI→DX→ADX→ADXR)，需自有 EMA/SMA 平滑 |
| **TSI** | `rust_ti::tsi` | 双重 EMA 平滑 + momentum，高度领域特定 |
| **VPT** | `rust_ti::vpt` | 累积逻辑 `prev + vol × (close-prev_close)/prev_close`，alpha 无对应 |
| **OBV** | `rust_ti::obv` | 涨加跌减的累积逻辑，纯领域算法 |
| **CMO** | `rust_ti::cmo` | 类似 RSI 的涨跌分离求和，alpha 无直接算子 |
| **PPO** | `rust_ti::ppo` | 基于两个 EMA 的差值比率，alpha EMA 初始化不同 |
| **A/D** | `rust_ti::accumulation_distribution` | CLV × Volume 累积，领域特定 |
| **PVI/NVI** | `rust_ti::pvi/nvi` | 条件累积，领域特定 |
| **RVI** | `rust_ti::rvi` | 四价加权（OHLC Symmetric）+ SMA 平滑，alpha 无对应 |
| **Ulcer Index** | `rust_ti::ulcer_index` | drawdown 百分比的 RMS，组合计算复杂 |

### 5.3 RTI → alpha 的推荐迁移

| # | 指标 | alpha 实现方式 | 优先级 | 收益 |
|---|------|--------------|--------|------|
| 1 | **Aroon Up/Down/Osc** | `ta_hhvbars(high, N)` + `ta_llvbars(low, N)` → 简单公式 | 🟢 高 | 消除 rust_ti 依赖、获得 NaN 处理和并行化 |
| 2 | **Stochastic %K** | `ta_hhv` + `ta_llv` → `(close - LLV)/(HHV - LLV)*100` | 🟢 高 | 复用已有 KDJ 的 HHV/LLV 逻辑 |
| 3 | **Williams %R** | `ta_hhv` + `ta_llv` → `-(HHV - close)/(HHV - LLV)*100` | 🟢 高 | 与 Stochastic 共享实现 |
| 4 | **ROC** | `ta_ref(close, 1)` → element-wise `(c - prev)/prev` | 🟡 中 | 收益不大，但统一风格 |
| 5 | **CCI** | `ta_ma` + 手写 MAD | 🟡 中 | 部分替换，MAD 仍需手写 |

---

## 总结对照表

| 问题 | 你的当前实现 | alpha 实现 | 行业标准 | 建议 |
|------|------------|-----------|---------|------|
| StdDev ddof | **0** (Population) | 1 (Sample) | **0** (Population) | ✅ 保持 ddof=0，给 alpha 增加 ddof 参数 |
| EMA 初始化 | **SMA 种子** | 首值种子 | **SMA 种子** | ✅ 不替换 EMA |
| RSI 平滑 | **Wilder's (1/N)** | 标准 EMA (2/(N+1)) | **Wilder's** | ✅ 不替换 RSI |
| ta_ref | N/A | 时序位移(shift) | N/A | 可用但无实质收益 |
| Aroon | rust_ti 委托 | `ta_hhvbars`/`ta_llvbars` | N/A | 🟢 **推荐替换** |
| Stochastic/%R | rust_ti 委托 | `ta_hhv`/`ta_llv` | N/A | 🟢 **推荐替换** |
