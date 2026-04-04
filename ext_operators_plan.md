# py-alpha-lib 扩展算子计划 — 纯增量策略

> **核心原则**：只做增量，不改存量。所有新增算子通过独立文件实现，原有算子文件一个字都不改。

---

## 背景

根据 `alpha_lib_refactor_analysis.md` 和 `technical_deep_dive.md` 的分析，`indicators-computation` 引擎需要将手写技术指标代码迁移到 `py-alpha-lib` 算子库。迁移前有 **3 个阻塞项**需要在 py-alpha-lib 侧先行解决。

本项目是 fork 仓库，原作者可能会有更新。为避免 merge 冲突，**所有变更必须是纯增量的**：

| ✅ 允许 | ❌ 禁止 |
|--------|---------|
| 新增文件 | 修改原有算子的内部逻辑 |
| 在 `mod.rs` 末尾追加行 | 修改原有函数签名 |
| 新增函数 | 给原有函数增加默认参数 |

---

## WASM 提交审计结论

昨天的提交 `33c06a5` (feat: 扩展支持非并行) 涉及 31 个文件的分析：

| 分类 | 文件数 | 冲突风险 | 结论 |
|------|--------|---------|------|
| 纯新增文件 | 6 | 🟢 零 | `par_compat.rs`, `quantile.rs` 等 |
| 机械式宏替换 | 19 | 🟡 中 | `rayon` 直调 → `par_for_each_N!` 宏 |
| 基础设施 feature gate | 6 | 🔴 高 | `Cargo.toml`, `context.rs`, `error.rs`, `lib.rs`, `build.rs`, `mod.rs` |

**结论**：Rust 要让代码在有/没有 rayon 时都编译通过，宏替换是唯一方案。19 个算子文件的改动不可避免，但都是相同模式（删 `use rayon::prelude::*` + 换宏），可通过 sed 脚本在 merge 后快速重新应用。**当前做法基本是最优方案。**

---

## 新增算子清单

### 1. `ta_stddev_pop` — Population Standard Deviation (ddof=0)

| 项目 | 值 |
|------|-----|
| **文件** | `src/algo/ext_stddev.rs` (新增) |
| **函数** | `ta_stddev_pop(ctx, r, input, periods)` |
| **语义** | 滚动总体标准差，除数为 N (ddof=0) |
| **对比** | 原 `ta_stddev` 使用除数 N-1 (ddof=1, Sample) |
| **用途** | Bollinger Bands、Rolling Volatility 的行业标准实现 |
| **冲突风险** | 🟢 零（纯新增文件） |

**数值示例**：
```
数据 [1, 2, 3], N=3, 均值=2:
ta_stddev_pop: σ = sqrt((1+0+1)/3)  = sqrt(0.667) = 0.8165  ← Population
ta_stddev:     s = sqrt((1+0+1)/2)  = sqrt(1.000) = 1.0000  ← Sample (不变)
```

**行业参考**：
| 平台 | ddof | 备注 |
|------|------|------|
| TA-Lib Bollinger | 0 | 行业标准 |
| TradingView | 0 | 行业标准 |
| Bloomberg | 0 | 行业标准 |
| Pandas rolling().std() | 1 | 统计学默认 |
| 原 ta_stddev | 1 | 跟随 Pandas |
| **新 ta_stddev_pop** | **0** | **跟随金融行业标准** |

**实现要点**：
- 逻辑从 `ta_stddev` 复制并修改除数部分（`N` 而非 `N-1`）
- 当 `N=1` 时返回 `0.0`（Population stddev 对 N=1 有定义且为 0）
- skip_nan 分支的有效计数条件：`no_nan_count >= 1`（原 `> 1`）
- non-skip_nan 分支：`periods >= 1`（原 `> 1`）
- 完整支持 `skip_nan` 和 `strictly_cycle` context flags
- 遵循 `par_for_each_2!` 宏的并行化模式

---

### 2. `ta_zscore_pop` — Population Z-Score (ddof=0)

| 项目 | 值 |
|------|-----|
| **文件** | `src/algo/ext_zscore.rs` (新增) |
| **函数** | `ta_zscore_pop(ctx, r, input, periods)` |
| **语义** | 滚动 Z-Score，内部标准差使用 ddof=0 |
| **对比** | 原 `ta_zscore` 内部使用 ddof=1 |
| **用途** | Volume Z-Score 指标的行业标准实现 |
| **冲突风险** | 🟢 零（纯新增文件） |

**实现要点**：
- 完全独立实现，不调用 `ta_zscore` 或 `ta_stddev`
- 方差除数用 `N`：`var = var_num / count`
- 有效计数条件：`no_nan_count >= 1`（Population 对 N=1 虽无意义但不报错）
  - 但实际使用中 zscore 需要 N >= 2 才有统计意义，此处保持 `>= 2` 与原算子一致
- 完整支持 `skip_nan` 和 `strictly_cycle` context flags

---

### 3. `ta_rank_pct` — 滚动排名百分位

| 项目 | 值 |
|------|-----|
| **文件** | `src/algo/ext_rank_pct.rs` (新增) |
| **函数** | `ta_rank_pct(ctx, r, input, periods)` |
| **语义** | 当前值在滚动窗口内的排名百分位 (0.0~1.0) |
| **对比** | `ta_quantile(data, N, q)` → 返回第 q 分位的数值（语义不同） |
| **用途** | Price Percentile (ML 特征 `price_percentile`) |
| **冲突风险** | 🟢 零（纯新增文件） |

**公式**：
```
rank_pct[i] = count(window_values <= data[i]) / valid_count_in_window
```

**数值示例**：
```
input = [10, 20, 30, 15, 25], periods=3, strictly_cycle

idx 0: NaN   (窗口不足)
idx 1: NaN   (窗口不足)
idx 2: [10,20,30] → count(<=30)/3 = 3/3 = 1.000
idx 3: [20,30,15] → count(<=15)/3 = 1/3 = 0.333
idx 4: [30,15,25] → count(<=25)/3 = 2/3 = 0.667
```

**实现要点**：
- 与 `ta_quantile` 相同的窗口管理基础设施（SkipNanWindow）
- 核心计算：遍历窗口内所有有效值，计数 `<= current` 的个数，除以有效值总数
- 不需要排序（O(N) 而非 O(N·log(N))），性能优于排序再查找
- 完整支持 `skip_nan` 和 `strictly_cycle` context flags

---

## 文件变更汇总

| 操作 | 文件 | 冲突风险 |
|------|------|---------|
| **新增** | `src/algo/ext_stddev.rs` | 🟢 零 |
| **新增** | `src/algo/ext_zscore.rs` | 🟢 零 |
| **新增** | `src/algo/ext_rank_pct.rs` | 🟢 零 |
| **追加** | `src/algo/mod.rs` (末尾追加 6 行) | 🟢 极低 |

```diff
 // mod.rs 末尾追加:
+// === Extension modules (fork-local, additive only) ===
+mod ext_stddev;
+mod ext_zscore;
+mod ext_rank_pct;
+
+pub use ext_stddev::*;
+pub use ext_zscore::*;
+pub use ext_rank_pct::*;
```

---

## Python 绑定

`build.rs` 会自动扫描所有 `ta_` 前缀的 `pub fn`，自动生成 Python 绑定。
新增的 `ta_stddev_pop`、`ta_zscore_pop`、`ta_rank_pct` 将被自动发现和注册，**无需修改 build.rs**。

---

## DRY 取舍说明

`ta_stddev_pop` 与原 `ta_stddev` 约 90% 代码重复（仅除数 `N` vs `N-1` 不同）。
`ta_zscore_pop` 与原 `ta_zscore` 约 85% 代码重复。

> **这是刻意为之的设计决策**：牺牲 DRY 换取零合并冲突。
> 如果要提取公共 helper 来消除重复，就必须修改原 `stddev.rs` / `zscore.rs` 来导出 helper，违反「不改存量」原则。
> 在 fork 维护场景下，「零冲突 > 零重复」。

---

## 验证计划

```bash
# 1. 原生编译 + 全量测试（含新算子测试）
cargo test

# 2. 无默认 feature 编译测试（验证 WASM 兼容）
cargo test --no-default-features

# 3. 仅 parallel feature 测试
cargo test --features parallel --no-default-features
```

### 数值验证对照表

| 算子 | 输入 | 期望输出 |
|------|------|---------|
| `ta_stddev_pop([1,2,3], 3)` | 完整窗口 | `[NaN, NaN, 0.8165]` (strictly) |
| `ta_stddev([1,2,3], 3)` | 完整窗口 | `[NaN, NaN, 1.0]` (不变) |
| `ta_zscore_pop([1,2,3,4,5], 3)` | 滚动 | zscore 值与 ddof=0 标准差匹配 |
| `ta_rank_pct([10,20,30,15,25], 3)` | 滚动 | `[NaN, NaN, 1.0, 0.333, 0.667]` |

---

## 下游影响

`indicators-computation` 引擎迁移时使用：

| 场景 | 使用算子 |
|------|---------|
| Bollinger σ 部分 | `ta_stddev_pop` (Population, ddof=0) |
| BBW / PctB / Deviation | downstream 组合 `ta_ma` + `ta_stddev_pop` |
| volume_zscore | `ta_zscore_pop` |
| volatility (日收益率标准差) | 先手算日收益率序列 → `ta_stddev_pop` |
| price_percentile | `ta_rank_pct` |
| volume_trend | `ta_slope` + `ta_ma` (已有, 不需要新算子) |
| drawdown_from_high | `ta_hhv` (已有, 不需要新算子) |
| volume_ratio / ATR SMA | `ta_ma` (已有, 不需要新算子) |

---

# P2/P3 扩展算子规划

> 以下算子**当前不紧急**，但如果实现可以解锁更多 indicators-computation 指标的迁移。
> 按照同样的纯增量原则执行。

---

## P2：SMA 种子 EMA 系列 — 解锁 EMA / MACD / RSI

### 背景

原 `ta_ema` 使用**首值种子**（`prev = input[0]`），但金融行业标准是 **SMA 种子**（前 N 值的 SMA 作为初始值，从 index=N-1 开始输出）。这个差异导致 EMA / MACD / RSI 无法迁移。

如果提供 SMA 种子版本，这三个指标的迁移路径将全部打通。

### [NEW] `src/algo/ext_ema.rs`

提供 **2 个对外函数 + 1 个内部实现**：

#### 内部实现（不对外暴露）

```rust
/// 通用 SMA-seeded DMA
/// - 前 sma_periods 个值计算 SMA 作为种子
/// - 从 index=sma_periods-1 开始输出
/// - EMA 递推: result[i] = alpha * input[i] + (1-alpha) * result[i-1]
fn dma_sma_seeded_impl<NumT: Float + Send + Sync>(
  ctx: &Context, r: &mut [NumT], input: &[NumT],
  alpha: NumT, sma_periods: usize,
) -> Result<(), Error>
```

#### 对外函数 1：`ta_ema_sma_seeded`

```rust
/// SMA-seeded EMA (行业标准 EMA)
///
/// α = 2 / (periods + 1), SMA 种子取前 periods 个值
///
/// 对比原 ta_ema（首值种子）：
///   ta_ema:            idx 0 就输出，种子 = input[0]
///   ta_ema_sma_seeded: idx N-1 开始输出，种子 = SMA(input[0..N])
///
/// 匹配：TA-Lib / TradingView / Bloomberg 的 EMA 实现
pub fn ta_ema_sma_seeded<NumT: Float + Send + Sync>(
  ctx: &Context, r: &mut [NumT], input: &[NumT], periods: usize,
) -> Result<(), Error>
```

#### 对外函数 2：`ta_wilder_smooth`

```rust
/// Wilder's Smoothing (Wilder 平滑)
///
/// α = 1 / periods, SMA 种子取前 periods 个值
///
/// 公式: result[i] = (result[i-1] * (N-1) + input[i]) / N
/// 等价: result[i] = (1/N) * input[i] + (1 - 1/N) * result[i-1]
///
/// 用途: RSI 的 avg_gain/avg_loss 平滑; ATR (Wilder版); ADX 的 DI 平滑
///
/// 匹配：TA-Lib / TradingView 的 RSI / ATR 平滑方式
pub fn ta_wilder_smooth<NumT: Float + Send + Sync>(
  ctx: &Context, r: &mut [NumT], input: &[NumT], periods: usize,
) -> Result<(), Error>
```

### 数值示例

```
输入: [10, 11, 12, 13, 14], periods=3

ta_ema (首值种子, α=0.5):
  [10.0, 10.5, 11.25, 12.125, 13.0625]

ta_ema_sma_seeded (SMA种子, α=0.5):
  [NaN, NaN, 11.0, 12.0, 13.0]
  种子 = SMA(10,11,12) = 11.0

ta_wilder_smooth (SMA种子, α=1/3):
  [NaN, NaN, 11.0, 11.667, 12.444]
  种子 = SMA(10,11,12) = 11.0
  idx3: (11.0*2 + 13)/3 = 11.667
  idx4: (11.667*2 + 14)/3 = 12.444
```

---

### RSI 的完整替换路径

有了 `ta_wilder_smooth`，RSI 在 indicators-computation 侧变为纯组合：

```
// 步骤 1: 价格变化
diff = close - ta_ref(close, 1)           // 可用 ta_ref，或调用方直接手算

// 步骤 2: gains/losses 分离（调用方 element-wise 操作）
gains[i]  = max(0,  diff[i])              // 涨幅
losses[i] = max(0, -diff[i])              // 跌幅绝对值

// 步骤 3: Wilder 平滑（alpha-lib 算子）
avg_gains  = ta_wilder_smooth(gains,  N)  // ← 核心！SMA种子 + α=1/N
avg_losses = ta_wilder_smooth(losses, N)

// 步骤 4: RS 和 RSI（调用方 element-wise 操作）
RS  = avg_gains / avg_losses
RSI = 100 - 100 / (1 + RS)
```

> **gains/losses 分离不需要新算子**。这是基础的 element-wise 数学操作（`max(0, x)`），
> 不涉及滚动窗口或时序计算，在调用方用一行 for 循环或 NumPy 即可完成。
> 放在算子库里既不符合库的设计哲学（`ta_` 前缀代表时序算子），也增加了不必要的内存分配。

### MACD 的替换路径

```
fast_ema   = ta_ema_sma_seeded(close, fast_period)     // e.g. 12
slow_ema   = ta_ema_sma_seeded(close, slow_period)     // e.g. 26
macd_line  = fast_ema - slow_ema                        // element-wise
signal     = ta_ema_sma_seeded(macd_line, signal_period) // e.g. 9
histogram  = macd_line - signal                         // element-wise
```

---

## P3：Mean Absolute Deviation — 解锁 CCI

### [NEW] `src/algo/ext_mad.rs`

```rust
/// Rolling Mean Absolute Deviation (MAD)
///
/// MAD = mean(|x - mean|) 在滚动窗口内
///
/// 类似 ta_moment(k=2) 的实现，但用 |x - mean| 代替 (x - mean)^2
/// 注意：ta_moment(k=1) = mean(x - mean) 恒等于 0，不是 MAD
///
/// 用途: CCI 的分母部分
///   CCI = (TP - SMA(TP, N)) / (0.015 × MAD(TP, N))
pub fn ta_mad<NumT: Float + Send + Sync>(
  ctx: &Context, r: &mut [NumT], input: &[NumT], periods: usize,
) -> Result<(), Error>
```

### CCI 的替换路径

```
TP  = (high + low + close) / 3           // 调用方 element-wise
sma = ta_ma(TP, N)                        // alpha-lib (已有)
mad = ta_mad(TP, N)                       // alpha-lib (P3 新增)
CCI = (TP - sma) / (0.015 * mad)          // 调用方 element-wise
```

---

## P2/P3 文件影响汇总

| 优先级 | 操作 | 文件 | 冲突风险 |
|--------|------|------|---------|
| P2 | **新增** | `src/algo/ext_ema.rs` | 🟢 零 |
| P3 | **新增** | `src/algo/ext_mad.rs` | 🟢 零 |
| P2+P3 | **追加** | `src/algo/mod.rs` (末尾追加 4 行) | 🟢 极低 |

```diff
 // mod.rs 末尾追加 (P2+P3 完成时):
+mod ext_ema;
+mod ext_mad;
+pub use ext_ema::*;
+pub use ext_mad::*;
```

---

## 指标迁移全景图（含 P1/P2/P3 后）

| 指标 | 迁移前状态 | 迁移后来源 | 所需 P 级 |
|------|-----------|-----------|----------|
| SMA | ✅ 已迁移 | `ta_ma` | — |
| WMA | ✅ 已迁移 | `ta_lwma` | — |
| KDJ | ✅ 已迁移 | `ta_hhv` + `ta_llv` | — |
| Slope OLS | ✅ 已迁移 | `ta_slope` | — |
| Bollinger σ | 🔜 待迁移 | `ta_stddev_pop` | P1 ✅ |
| Volume Z-Score | 🔜 待迁移 | `ta_zscore_pop` | P1 ✅ |
| Price Percentile | 🔜 待迁移 | `ta_rank_pct` | P1 ✅ |
| Volatility | 🔜 待迁移 | `ta_stddev_pop` | P1 ✅ |
| Volume Trend | 🔜 待迁移 | `ta_slope` + `ta_ma` | — |
| Drawdown | 🔜 待迁移 | `ta_hhv` | — |
| Volume Ratio | 🔜 待迁移 | `ta_ma` | — |
| ATR SMA | 🔜 待迁移 | `ta_ma` | — |
| PSY | 🔜 待迁移 | `ta_ref` + `ta_sum` | — |
| Aroon | 🔜 待迁移 (from RTI) | `ta_hhvbars` + `ta_llvbars` | — |
| Stochastic/%R | 🔜 待迁移 (from RTI) | `ta_hhv` + `ta_llv` | — |
| **EMA** | ❌ → 🔜 | `ta_ema_sma_seeded` | **P2** |
| **MACD** | ❌ → 🔜 | `ta_ema_sma_seeded` 组合 | **P2** |
| **RSI** | ❌ → 🔜 | `ta_wilder_smooth` + 调用方组合 | **P2** |
| **CCI** | ❌ → 🔜 (from RTI) | `ta_ma` + `ta_mad` | **P3** |
| OBV/ARBR/VR/OSC | ❌ 保持不变 | 领域特定逻辑 | — |
| SAR/ADX/TSI/VPT | ❌ 保持不变 | rust_ti 委托 | — |
| Patterns层 | ❌ 不适用 | 非时序算子 | — |
