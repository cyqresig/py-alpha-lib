# py-alpha-lib Feature-Gate 改造实施计划

> 策略: "算子下沉，指标上浮"
> 目标: 将 py-alpha-lib 改造为可被 `indicators-computation` 作为纯 Rust 库依赖的 feature-gated crate

---

## Phase 1: Feature-Gate 改造 (预计 1-2 天)

### 1.1 Cargo.toml 依赖 optional 化

将所有非核心依赖改为 optional，通过 features 控制：

```toml
[features]
default = ["parallel", "python"]
parallel = ["rayon"]
python = ["pyo3", "numpy", "pyo3-log"]

[dependencies]
rayon         = { version = "1.11", optional = true }
pyo3          = { version = "0.28", features = ["abi3"], optional = true }
numpy         = { version = "0.28", optional = true }
pyo3-log      = { version = "0.13", optional = true }

# 始终需要
num-traits = "0.2"
log = "0.4"
thiserror = "2"
```

**注意**: `crate-type` 需要条件化:
- `python` feature → `cdylib` + `rlib`
- 无 `python` → 仅 `rlib`

---

### 1.2 algo/*.rs — rayon 条件编译 (19 个文件)

#### 方案 A: 辅助宏 (推荐，减少重复代码)

新建 `src/algo/par_compat.rs`，定义统一宏:

```rust
/// 双输入并行/串行切换宏
macro_rules! par_for_each_2 {
    ($r:expr, $input:expr, $chunk_size:expr, |$r_name:ident, $x_name:ident| $body:block) => {{
        #[cfg(feature = "parallel")]
        {
            use rayon::prelude::*;
            $r.par_chunks_mut($chunk_size)
                .zip($input.par_chunks($chunk_size))
                .for_each(|($r_name, $x_name)| $body);
        }
        #[cfg(not(feature = "parallel"))]
        {
            $r.chunks_mut($chunk_size)
                .zip($input.chunks($chunk_size))
                .for_each(|($r_name, $x_name)| $body);
        }
    }};
}

/// 三输入并行/串行切换宏 (如 cross, sumif, stats 等)
macro_rules! par_for_each_3 {
    ($r:expr, $a:expr, $b:expr, $chunk_size:expr, |$r_name:ident, $a_name:ident, $b_name:ident| $body:block) => {{
        #[cfg(feature = "parallel")]
        {
            use rayon::prelude::*;
            $r.par_chunks_mut($chunk_size)
                .zip($a.par_chunks($chunk_size))
                .zip($b.par_chunks($chunk_size))
                .for_each(|(($r_name, $a_name), $b_name)| $body);
        }
        #[cfg(not(feature = "parallel"))]
        {
            $r.chunks_mut($chunk_size)
                .zip($a.chunks($chunk_size))
                .zip($b.chunks($chunk_size))
                .for_each(|(($r_name, $a_name), $b_name)| $body);
        }
    }};
}

/// into_par_iter / into_iter 切换宏 (group.rs, rank.rs, neutralize.rs 等)
macro_rules! par_range_for_each {
    ($range:expr, |$j:ident| $body:block) => {{
        #[cfg(feature = "parallel")]
        {
            use rayon::prelude::*;
            $range.into_par_iter().for_each(|$j| $body);
        }
        #[cfg(not(feature = "parallel"))]
        {
            for $j in $range $body
        }
    }};
}
```

#### 需要改造的 19 个文件清单

| 文件 | rayon 使用方式 | 改造动作 |
|------|---------------|---------|
| `ma.rs` | `par_chunks_mut` + `par_chunks` | → `par_for_each_2!` |
| `ema.rs` | `par_chunks_mut` + `par_chunks` | → `par_for_each_2!` |
| `stddev.rs` | `par_chunks_mut` + `par_chunks` | → `par_for_each_2!` |
| `sum.rs` | `par_chunks_mut` + `par_chunks` (×3) | → `par_for_each_2!` + `par_for_each_3!` |
| `stats.rs` | `par_chunks_mut` + `par_chunks` (×5) | → `par_for_each_2!` + `par_for_each_3!` |
| `slope.rs` | `par_chunks_mut` + `par_chunks` | → `par_for_each_2!` |
| `rank.rs` | `par_chunks_mut` + `into_par_iter` (×3) | → `par_for_each_2!` + `par_range_for_each!` |
| `extremum.rs` | `par_chunks_mut` + `par_chunks` | → `par_for_each_2!` |
| `cross.rs` | `par_chunks_mut` + `par_chunks` (×3) | → `par_for_each_3!` |
| `scan.rs` | `par_chunks_mut` + `par_chunks` (×2) | → `par_for_each_3!` |
| `backfill.rs` | `par_chunks_mut` + `par_chunks` (×2) | → `par_for_each_2!` |
| `entropy.rs` | `par_chunks_mut` + `par_chunks` | → `par_for_each_2!` |
| `moments.rs` | `par_chunks_mut` + `par_chunks` | → `par_for_each_2!` |
| `zscore.rs` | `par_chunks_mut` + `into_par_iter` (×2) | → `par_for_each_2!` + `par_range_for_each!` |
| `misc.rs` | `par_chunks_mut` + `par_chunks` | → `par_for_each_2!` |
| `series.rs` | `par_chunks_mut` + `par_chunks` | → `par_for_each_2!` |
| `returns.rs` | `par_chunks_mut` + `par_chunks` | → `par_for_each_2!` |
| `neutralize.rs` | `into_par_iter` | → `par_range_for_each!` |
| `group.rs` | `into_par_iter` (×2) + `UnsafePtr` | → `par_range_for_each!` + 串行分支去 `UnsafePtr` |

---

### 1.3 lib.rs — PyO3 绑定条件编译

```rust
#[cfg(feature = "python")]
mod algo_impl { ... }  // 现有 PyO3 绑定代码

#[cfg(feature = "python")]
#[pymodule]
fn _algo(m: &Bound<'_, PyModule>) -> PyResult<()> { ... }
```

关键改动:
- `use pyo3::prelude::*;` → `#[cfg(feature = "python")] use pyo3::prelude::*;`
- `use rayon::iter::*;` → `#[cfg(feature = "parallel")] use rayon::iter::*;`
- `algo_impl` 模块整体加 `#[cfg(feature = "python")]`

---

### 1.4 build.rs — 代码生成器模板改造

`build.rs` 中 `into_par_iter` 出现约 10 处，全部在生成 PyO3 绑定的 List 分支中。

改造方式:
1. 整个 `build_py_bindings()` 函数加 `#[cfg(feature = "python")]` 条件（因为只有 Python 绑定才需要 List 并行）
2. 模板中 `into_par_iter` 替换为条件编译分支

---

### 1.5 验证

```bash
# 验证 1: 默认编译（带 parallel + python）— 行为不变
maturin develop
python -c "import alpha; print('ok')"

# 验证 2: 纯库编译（无 parallel，无 python）— indicators-computation 使用场景
cargo build --no-default-features

# 验证 3: 运行测试
cargo test --all-features
cargo test --no-default-features
```

---

## Phase 2: 扩展缺失算子 (预计 0.5-1 天)

遵循项目 `.agent/skills/add_algo/SKILL.md` 规范。

### 2.1 需要新增的算子

| 算子 | 签名 | 用途 |
|------|------|------|
| `ta_quantile` | `(ctx, r, input, periods, quantile: NumT)` | Alpha158 的 QTLU/QTLD 因子 |

### 2.2 通过现有算子组合即可的（无需新增）

| 目标 | 组合方式 |
|------|---------|
| R² (RSQR) | `1.0 - ta_var(regresi) / ta_var(close)` |
| RSI | `ta_ema(gain) / ta_ema(loss)` |
| MACD | `ta_ema(close, 12) - ta_ema(close, 26)` |
| BBANDS | `ta_ma(close) ± N * ta_stddev(close)` |
| ATR | `ta_ma(true_range)` |

---

## Phase 3: indicators-computation 依赖集成 (预计 2-3 天)

> 此阶段在 indicators-computation 项目中执行，不在 py-alpha-lib 中。

### 3.1 添加 Cargo 依赖

```toml
# indicators-computation/Cargo.toml
[dependencies]
alpha = { path = "../py-alpha-lib", default-features = false }
```

### 3.2 逐步重构指标

按优先级逐个指标重构，内部调用 `alpha::algo` 替代自有实现:

| 优先级 | 指标 | 当前实现 | 重构为 |
|:------:|------|---------|--------|
| P0 | SMA | `sma.rs` 自有 | `alpha::algo::ta_ma()` |
| P0 | EMA | `ema.rs` 自有 | `alpha::algo::ta_ema()` |
| P0 | WMA | `wma.rs` 自有 | `alpha::algo::ta_ma()` 变体 |
| P1 | RSI | `rsi.rs` 自有 | `alpha::algo::ta_ema()` 组合 |
| P1 | MACD | `macd.rs` 自有 | `alpha::algo::ta_ema()` 组合 |
| P1 | Bollinger | `bollinger.rs` 自有 | `alpha::algo::ta_ma()` + `ta_stddev()` |
| P1 | KDJ | `kdj.rs` 自有 | `alpha::algo::ta_hhv()` + `ta_llv()` + `ta_ema()` |
| P2 | BIAS | `bias.rs` 自有 | `alpha::algo::ta_ma()` 组合 |
| P2 | Slope | `slope.rs` 自有 | `alpha::algo::ta_slope()` |

**注意**: 以下模块**不参与重构**（py-alpha-lib 无对标能力）:
- `signal_resonance/` — 纯业务逻辑
- `patterns/` — Pivot/ZigZag/CUSUM/Divergence
- `rti/` — rust_ti 桥接层
- `grid/` — 参数网格搜索

### 3.3 关键设计决策

**Context 适配**: py-alpha-lib 的 `algo::Context` 与 indicators-computation 的调用约定不同:
- py-alpha-lib: `Context { _start, _end, _groups, _flags }` — 支持多 symbol 分组
- indicators-computation: 单个 symbol 调用

集成时统一使用 `Context::default()`（单 symbol 模式）即可。

---

## Phase 4: 验证 (预计 0.5 天)

### 4.1 编译验证

```bash
# indicators-computation 项目中
# PyO3 编译
maturin build --release

# WASM 编译
wasm-pack build --target web
```

### 4.2 数值一致性验证

对比重构前后的指标输出:
- 使用相同输入数据
- 对比 SMA/EMA/RSI/MACD/Bollinger 输出
- 允许 1e-10 精度误差

### 4.3 性能回归测试

确保重构后性能无显著退化:
- 单 symbol 计算性能
- WASM 端计算性能

---

## 风险与应对

| 风险 | 严重程度 | 应对策略 |
|------|:--------:|---------|
| `build.rs` 代码生成器修改 | 中 | 先理解模板逻辑，修改 ~6 处字符串模板 |
| `UnsafePtr` 串行分支多余 | 低 | 串行分支直接用安全代码，不走 `UnsafePtr` |
| `Context` API 差异 | 低 | 单 symbol 场景使用 `Context::default()` |
| Cargo crate-type 条件化 | 低 | 使用 `cfg` 或 build script 动态设置 |
| 测试矩阵翻倍 | 低 | CI 同时跑 `--all-features` 和 `--no-default-features` |

---

## 执行顺序建议

```
Phase 1.1 Cargo.toml → Phase 1.2 par_compat 宏 → Phase 1.2 algo/*.rs (逐文件)
    → Phase 1.3 lib.rs → Phase 1.4 build.rs → Phase 1.5 验证
        → Phase 2 扩展算子
            → Phase 3 indicators-computation 集成 (另一个项目)
                → Phase 4 最终验证
```
