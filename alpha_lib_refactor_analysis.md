# PyO3 暴露指标 → py-alpha-lib 算子替换可行性分析

## 现状概述

### 已完成迁移（4 个核心算子）
| 指标 | 源文件 | 使用的 alpha 算子 |
|------|--------|------------------|
| SMA | `indicators/sma.rs` | `ta_ma` |
| WMA | `indicators/wma.rs` | `ta_lwma` |
| KDJ (RSV部分) | `indicators/kdj.rs` | `ta_hhv`, `ta_llv` |
| Slope OLS | `indicators/slope.rs` | `ta_slope` |

### py-alpha-lib 算子库全量清单
| 模块 | 算子 | 功能 |
|------|------|------|
| `ma.rs` | `ta_ma`, `ta_product` | SMA, 滚动乘积 |
| `ema.rs` | `ta_ema`, `ta_sma (EMA variant)`, `ta_dma`, `ta_lwma` | EMA, 平滑MA, DMA, 线性加权MA |
| `stddev.rs` | `ta_stddev` | 滚动标准差 (Sample, ddof=1) |
| `slope.rs` | `ta_slope`, `ta_intercept`, `ta_corr` | OLS 斜率/截距/相关系数 |
| `extremum.rs` | `ta_hhv`, `ta_llv`, `ta_hhvbars`, `ta_llvbars` | 滚动最高/最低, bars since |
| `zscore.rs` | `ta_zscore`, `ta_cc_zscore` | 滚动 Z-Score, 横截面 Z-Score |
| `quantile.rs` | `ta_quantile` | 滚动百分位 |
| `series.rs` | `ta_ref`, `ta_barslast`, `ta_barssince`, `ta_count` | 位移/延迟, bars since, 滚动计数 |
| `sum.rs` | `ta_sum`, `ta_sumbars`, `ta_sumif` | 滚动求和, 条件求和 |
| `returns.rs` | `ta_fret` | 未来收益率(因子标签用) |
| `misc.rs` | `ta_min_max_diff`, `ta_weighted_delay`, `ta_moment` | 极差, 加权延迟, 高阶矩 |
| `rank.rs` | 排名相关 | 横截面排名 |
| `cross.rs` | 交叉检测 | 金叉/死叉 |
| `entropy.rs` | 信息熵 | 滚动信息熵 |
| `backfill.rs` | 缺失值填充 | NaN 回填 |
| `neutralize.rs` | 中性化 | 截面中性化 |

---

## 可替换分析

### ✅ 高优先级 — 直接替换，逻辑完全匹配

#### 1. Bollinger Bands 标准差部分
| 项目 | 值 |
|------|-----|
| **当前实现** | `indicators/bollinger.rs` — 手写 for 循环计算窗口标准差 |
| **alpha 算子** | `ta_stddev` — 滚动标准差 |
| **影响范围** | `compute_bollinger_py`, `compute_bbw_py`, `compute_bollinger_pct_b_py`, `compute_boll_deviation_py` |
| **注意点** | alpha 用 **ddof=1** (Sample)，当前代码用 **ddof=0** (Population)。需确认是否接受微小数值差异，或为 alpha 增加 ddof 参数 |

#### 2. Volume Z-Score
| 项目 | 值 |
|------|-----|
| **当前实现** | `ml_features/volume.rs::compute_volume_zscore` — 手写滚动均值+标准差+zscore |
| **alpha 算子** | `ta_zscore` — 一步到位的滚动 Z-Score |
| **影响范围** | ML 特征 `volume_zscore` |
| **注意点** | alpha 的 ddof=1, 当前代码 ddof=0。需统一 |

#### 3. Price Percentile (百分位)
| 项目 | 值 |
|------|-----|
| **当前实现** | `ml_features/price.rs::compute_price_percentile` — 手写窗口内 `count(<=x)/total` |
| **alpha 算子** | `ta_quantile` — 滚动百分位 |
| **影响范围** | ML 特征 `price_percentile` |
| **注意点** | 语义不同：当前实现是「排名百分位」(rank-based)，alpha 的 `ta_quantile` 是「分位数值」。**不可直接替换**，但可以用 `ta_hhvbars` / `ta_llvbars` 或新增 `ta_rank_pct` 来实现 |

#### 4. Volume Trend (OLS 斜率)
| 项目 | 值 |
|------|-----|
| **当前实现** | `ml_features/volume.rs::compute_volume_trend` — 手写 OLS slope / mean |
| **alpha 算子** | `ta_slope` — OLS 线性回归斜率 + `ta_ma` — 均值 |
| **影响范围** | ML 特征 `volume_trend` |
| **注意点** | 最终值 = `ta_slope(volume, period) / ta_ma(volume, period)`，组合两个算子即可 |

#### 5. Rolling Volatility (滚动收益率标准差)
| 项目 | 值 |
|------|-----|
| **当前实现** | `ml_features/price.rs::compute_rolling_volatility` — 手写日收益率 → 滚动标准差 |
| **alpha 算子** | 先手工计算日收益率序列，再用 `ta_stddev` 求滚动标准差 |
| **影响范围** | ML 特征 `volatility` |
| **注意点** | ddof 差异同上 |

#### 6. Volume Ratio (量比)
| 项目 | 值 |
|------|-----|
| **当前实现** | `ml_features/volume.rs::compute_volume_ratio` — 手写 `volume[i] / avg(window)` |
| **alpha 算子** | `ta_ma` — 计算滚动均值，然后 element-wise 除法 |
| **影响范围** | ML 特征 `volume_ratio` |

#### 7. Drawdown from High
| 项目 | 值 |
|------|-----|
| **当前实现** | `ml_features/price.rs::compute_drawdown_from_high` — 手写滚动 max_high |
| **alpha 算子** | `ta_hhv` — 滚动最高值 |
| **影响范围** | ML 特征 `drawdown_from_high` |
| **注意点** | `drawdown = (close - ta_hhv(high, lookback)) / ta_hhv(high, lookback)` |

#### 8. ATR 的 SMA 部分
| 项目 | 值 |
|------|-----|
| **当前实现** | `ml_features/batch.rs::compute_atr_simple` — 手写 True Range → SMA |
| **alpha 算子** | 手写 TR 序列后用 `ta_ma` 计算 SMA |
| **影响范围** | ML 特征 `atr_14` |

### 🟡 中优先级 — 可替换但需组合/适配

#### 9. Bollinger Bands (完整重构)
将 `compute_bollinger` 改为:
```
middle = ta_ma(close, period)
std = ta_stddev(close, period)  // 需 ddof=0 版本
upper = middle + k * std
lower = middle - k * std
```
> middle 已经用 `ta_ma`，只需把标准差那段手写 for 循环替换为 `ta_stddev`

#### 10. BBW (Bollinger Band Width)
当前实现依赖 `compute_bollinger`, 如果 Bollinger 迁移完成，BBW 自动受益

#### 11. PSY (心理线)
| 项目 | 值 |
|------|-----|
| **当前实现** | `indicators/psy.rs` — 手写滚动窗口统计上涨天数 |
| **alpha 算子** | 可用 `ta_count` (滚动布尔计数)：先计算 `close[i] > close[i-1]` 的布尔序列，再 `ta_count(condition, period)` |
| **影响范围** | `compute_psy_py`, ML 特征 `psy_12` |

#### 12. Turnover Rate Approx
| 项目 | 值 |
|------|-----|
| **当前实现** | `ml_features/volume.rs::compute_turnover_rate_approx` — 手写 `vol*close / avg(window)` |
| **alpha 算子** | 先计算 `amount = volume × close` 序列，再 `ta_ma(amount, period)` 获取均值，最后 element-wise 除法 |

#### 13. Return (N 日收益率)
| 项目 | 值 |
|------|-----|
| **当前实现** | `ml_features/price.rs::compute_return` — 手写 `(close[i] - close[i-N]) / close[i-N]` |
| **alpha 算子** | 可用 `ta_ref` (延迟) 获取 `close[i-N]`，然后元素级运算 |
| **影响范围** | ML 特征 `return_1`, `return_2`, `return_3` |
| **注意点** | alpha 没有直接的 `pct_change` 算子，需 `ta_ref` + element-wise 组合。收益不大，手写逻辑已经足够简单 |

### 🔴 低优先级 / 不建议替换

#### 14. EMA
| 原因 | alpha 的 `ta_ema` 初始化逻辑为 `prev = input[0]`，当前 `indicators/ema.rs` 使用第一个非 NaN 值初始化。**初始化语义不同**，强行替换会引入数值差异 |

#### 15. MACD
| 原因 | 基于 EMA 构建，继承 EMA 的初始化问题。且 MACD 有自定义 `multiply_factor` 参数，alpha 无直接对应 |

#### 16. RSI
| 原因 | 使用 Wilder's smoothing (EMA variant with alpha=1/N)，alpha 的 `ta_ema` 使用 alpha=2/(N+1)。**平滑系数根本不同**，无法替换 |

#### 17. OBV
| 原因 | 累积逻辑 (涨加跌减) 是领域特定算法，alpha 无对应算子 |

#### 18. ARBR / VR / OSC
| 原因 | 领域特定的复合指标(涨跌分类求和)，alpha 无直接对应。可以用 `ta_sumif` 部分重构，但收益有限 |

#### 19. CCI / MFI / Aroon / SAR / ADX / TSI / VPT
| 原因 | 已委托 RTI (rust_ti crate) 实现，不属于自研手写代码 |

#### 20. Pivot / ZigZag / CUSUM / 信号共振
| 原因 | 模式识别/事件检测逻辑，非时序计算算子，alpha-lib 完全不覆盖 |

---

## 汇总矩阵

| # | 指标/特征 | 当前来源 | 可用 alpha 算子 | 替换优先级 | 收益评估 |
|---|----------|---------|----------------|-----------|---------|
| 1 | Bollinger σ | 手写 | `ta_stddev` | 🟢 高 | 消除 ~20 行标准差 for 循环 |
| 2 | volume_zscore | 手写 | `ta_zscore` | 🟢 高 | 消除 ~40 行手写 zscore |
| 3 | volume_trend | 手写 OLS | `ta_slope` + `ta_ma` | 🟢 高 | 消除 ~40 行 OLS 重复代码 |
| 4 | volatility | 手写 | `ta_stddev`(对日收益率) | 🟢 高 | 消除 ~30 行手写代码 |
| 5 | drawdown_from_high | 手写 | `ta_hhv` | 🟢 高 | 消除 ~20 行滚动 max 循环 |
| 6 | volume_ratio | 手写 | `ta_ma` | 🟡 中 | 消除 ~15 行，但逻辑已简单 |
| 7 | ATR (SMA of TR) | 手写 | `ta_ma` (对 TR 序列) | 🟡 中 | 消除 ~25 行 |
| 8 | PSY | 手写 | `ta_count` | 🟡 中 | 需先构造布尔序列 |
| 9 | turnover_rate | 手写 | `ta_ma` | 🟡 中 | 同 volume_ratio |
| 10 | return_1/2/3 | 手写 | `ta_ref` + 元素运算 | 🟡 低 | 手写已足够简单(~10 行) |
| 11 | price_percentile | 手写 | 无直接对应 | ❌ 不可 | 语义不匹配 |
| 12 | EMA | 手写 | `ta_ema` | ❌ 不建议 | 初始化语义不同 |
| 13 | MACD | 手写 | 无直接对应 | ❌ 不建议 | 依赖自定义 EMA |
| 14 | RSI | 手写 | 无直接对应 | ❌ 不建议 | Wilder 平滑系数不同 |
| 15 | OBV/ARBR/VR/OSC | 手写 | 无/部分对应 | ❌ 不建议 | 领域特定逻辑 |
| 16 | CCI/MFI/Aroon/etc | RTI 委托 | N/A | ⬜ 已委托 | 非自研代码 |
| 17 | Patterns层 | 自研 | N/A | ⬜ 不适用 | 非时序算子范畴 |

---

## 建议下一步

> [!IMPORTANT]
> **ddof 问题是首要障碍**：当前手写代码使用 Population StdDev (ddof=0)，alpha 使用 Sample StdDev (ddof=1)。
> 建议在 alpha 的 `ta_stddev` 中增加一个 `ddof` 参数（或新增 `ta_stddev_pop`），以避免引入数值差异。

**推荐迁移顺序：**
1. 解决 `ta_stddev` 的 ddof 问题
2. Bollinger σ → `ta_stddev`（影响 Bollinger/BBW/PctB/Deviation 全部下游）
3. `drawdown_from_high` → `ta_hhv`
4. `volume_zscore` → `ta_zscore`（需同步解决 ddof）
5. `volume_trend` → `ta_slope` + `ta_ma`
6. `volatility` → `ta_stddev`（对日收益率序列）
7. `volume_ratio` / `ATR SMA` → `ta_ma`（低风险收尾）
