# Feature Request: `py-alpha-lib` 增加单序列时间回归变体 (R-Squared, Residual)

## 1. 背景 (Background)
在量化技术分析与特征提取（尤其是类似于 Qlib Alpha158 这种因子的构建）中，经常需要对一组连续时间片上的状态数据（例如收盘价 `Close`）**基于时间推移**进行线性回归。
此时，线型回归的自变量 $X$ 通常是相对于滚动窗口内部的一个离散索引，即 $X = [0, 1, 2, ..., w-1]$。

## 2. 现状 (Current Status)
目前的 `py-alpha-lib` 库对回归提供了以下能力：
* **支持对时间回归并计算斜率**：`ta_slope` 算子只需要传入单序列 `y`，会自动对默认时间序列索引 $X$ 完成回归并输出斜率（Slope）。
* **不支持对时间回归并计算决定系数和残差**：对于回归计算的另外两个常用衍生物——决定系数（R-Squared）和残差（Residuals），库中只提供了双序列（Cross-Regression）的计算版本。
  * 比如由 `ta_regresi(ctx, r, y, x, periods)` 提供的残差计算，必须要显式传入一个独立等长的数组 `x`。

## 3. 问题与痛点 (Pain Points)
为了实现 Alpha158 中的 `RESI` 和 `RSQR`，如果我们强行复用当前的 `py-alpha-lib` 的双序列双变量接口：
1. **内存浪费严重**：在长周期的回测及庞大的 K 线数量前提下，我们必须由引擎额外显式在内存中平铺生成一个包含时间 Index 循环递增的巨大虚假数组 $X$，仅仅用于填补这个 API。
2. **性能受损**：时间序列的 OLS 公式原本可以由于自变量 $X$ 为等距自然数而被大幅简化，其方差、协方差皆可利用公式在 $O(1)$ 的状态下迭代得到。传入冗余数组打破了这层优化，白白消耗大量 CPU 计算去遍历并做乘加。

## 4. 改进建议 (Proposal)
为了填补算子的逻辑缺失并优化引擎性能，建议 `py-alpha-lib` 上游库能够直接补充对于时间序列特定回归的算子：

添加如下新接口（签名与 `ta_slope` 相似，只传 `y` 即可）：
```rust
/// 计算时间序列线性回归的决定系数 $R^2$ (单序列输入)
pub fn ta_rsqr<NumT: Float + Send + Sync>(
  ctx: &Context,
  r: &mut [NumT],
  y: &[NumT],
  periods: usize,
) -> Result<(), Error>;

/// 计算时间序列线性回归的残差 $\epsilon$ (单序列输入)
pub fn ta_resi<NumT: Float + Send + Sync>(
  ctx: &Context,
  r: &mut [NumT],
  y: &[NumT],
  periods: usize,
) -> Result<(), Error>;
```

## 5. 可参考的实现方案 (Reference Implementation)
在目前的 `indicators-computation/src/alpha158/rsqr.rs` 模块中，我们通过常数空间手动计算窗口数据的 OLS 来代替 `ta_regresi` 和 `ta_rsquare` 函数的调用。
该底层逻辑极其紧凑，利用 $Y$ 的均值结合等距 $X = [0..w-1]$ 的数学性质，极大压缩了计算步骤。我们可以直接将其逻辑无缝封装为上述建议的 `ta_rsqr` 与 `ta_resi`，从而完美闭环 `py-alpha-lib` 处理量化工式体系中的回归场景需求。
