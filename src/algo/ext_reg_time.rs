// Copyright 2026 MSD-RS Project LiJia
// SPDX-License-Identifier: BSD-2-Clause

use num_traits::Float;

use crate::algo::{Context, Error, is_normal, skip_nan_window::SkipNanWindow};

/// Internal core for single-series time regression.
///
/// Performs OLS regression of `y` against an implicit time index `x = [0, 1, ..., w-1]`
/// over a rolling window. The closure `op` receives pre-computed statistics and
/// returns the desired derived quantity (R², etc.).
///
/// Parameters of `op`:
///   `(n, sum_x, sum_x2, sum_y, sum_y2, sum_xy)` → `result`
///
/// This is a fork-local copy of the upstream `linear_reg_core` from `slope.rs`,
/// kept private to respect the additive-only change policy.
fn time_reg_core<NumT, F>(
  ctx: &Context,
  r: &mut [NumT],
  input: &[NumT],
  periods: usize,
  op: F,
) -> Result<(), Error>
where
  NumT: Float + Send + Sync,
  F: Fn(NumT, NumT, NumT, NumT, NumT, NumT) -> NumT + Sync + Send + Copy,
{
  if r.len() != input.len() {
    return Err(Error::LengthMismatch(r.len(), input.len()));
  }

  let r = ctx.align_end_mut(r);
  let input = ctx.align_end(input);

  if periods < 2 {
    r.fill(NumT::nan());
    return Ok(());
  }

  par_for_each_2!(r, input, ctx.chunk_size(r.len()), |r, x| {
      let start = ctx.start(r.len());
      r.fill(NumT::nan());

      if ctx.is_skip_nan() {
        let iter = SkipNanWindow::new(x, periods, start);
        let mut sum_y = NumT::zero();
        let mut sum_y2 = NumT::zero();
        let mut sum_xy_1based = NumT::zero();
        let mut count = 0;

        for i in iter {
          for k in i.prev_start..i.start {
            let old = x[k];
            if is_normal(&old) {
              sum_xy_1based = sum_xy_1based - sum_y;
              sum_y = sum_y - old;
              sum_y2 = sum_y2 - old * old;
              count -= 1;
            }
          }

          let val = x[i.end];
          if is_normal(&val) {
            count += 1;
            let n_t = NumT::from(count).unwrap();
            sum_y = sum_y + val;
            sum_y2 = sum_y2 + val * val;
            sum_xy_1based = sum_xy_1based + n_t * val;
          }

          if !is_normal(&val) {
            continue;
          }

          let mut should_output = true;
          if ctx.is_strictly_cycle() {
            if count != periods || (i.end - i.start + 1) != periods {
              should_output = false;
            }
          }

          if should_output && count >= 2 {
            let n = NumT::from(count).unwrap();
            let sum_x = n * (n - NumT::one()) / NumT::from(2.0).unwrap();
            let sum_x2 = n * (n - NumT::one()) * (NumT::from(2.0).unwrap() * n - NumT::one())
              / NumT::from(6.0).unwrap();

            let sum_xy = sum_xy_1based - sum_y;

            r[i.end] = op(n, sum_x, sum_x2, sum_y, sum_y2, sum_xy);
          }
        }
      } else {
        let mut sum_y = NumT::zero();
        let mut sum_y2 = NumT::zero();
        let mut sum_xy_1based = NumT::zero();
        let mut count = 0;
        let mut nan_in_window = 0;

        let pre_fill_start = if start >= periods { start - periods } else { 0 };
        for k in pre_fill_start..start {
          let val = x[k];
          if is_normal(&val) {
            count += 1;
            let n_t = NumT::from(count).unwrap();
            sum_y = sum_y + val;
            sum_y2 = sum_y2 + val * val;
            sum_xy_1based = sum_xy_1based + n_t * val;
          } else {
            count += 1;
            nan_in_window += 1;
          }
        }

        let total = r.len();
        for (n, (r, c)) in r
          .iter_mut()
          .zip(x.iter())
          .enumerate()
          .skip(ctx.start(total))
        {
          count += 1;
          let n_t = NumT::from(count).unwrap();

          if is_normal(c) {
            sum_y = sum_y + *c;
            sum_y2 = sum_y2 + *c * *c;
            sum_xy_1based = sum_xy_1based + n_t * *c;
          } else {
            nan_in_window += 1;
          }

          if count > periods {
            let old_idx = n - periods;
            let old = x[old_idx];

            sum_xy_1based = sum_xy_1based - sum_y;
            if is_normal(&old) {
              sum_y = sum_y - old;
              sum_y2 = sum_y2 - old * old;
            } else {
              nan_in_window -= 1;
            }
            count -= 1;
          }

          if count == periods {
            if nan_in_window > 0 {
              *r = NumT::nan();
            } else if ctx.is_strictly_cycle() && n < periods - 1 {
              *r = NumT::nan();
            } else {
              let n_val = NumT::from(periods).unwrap();
              let sum_x = n_val * (n_val - NumT::one()) / NumT::from(2.0).unwrap();
              let sum_x2 =
                n_val * (n_val - NumT::one()) * (NumT::from(2.0).unwrap() * n_val - NumT::one())
                  / NumT::from(6.0).unwrap();

              let sum_xy = sum_xy_1based - sum_y;

              *r = op(n_val, sum_x, sum_x2, sum_y, sum_y2, sum_xy);
            }
          } else {
            *r = NumT::nan();
          }
        }
      }
  });
  Ok(())
}

/// Time-Series Linear Regression R-Squared (Coefficient of Determination)
///
/// Calculates R² for a rolling OLS regression of `y` against the implicit
/// time index `x = [0, 1, ..., w-1]`.
///
/// R² = (n·Σxy − Σx·Σy)² / ((n·Σx² − (Σx)²) · (n·Σy² − (Σy)²))
///
/// This is a single-series variant that avoids allocating an explicit `x` array.
pub fn ta_rsqr<NumT: Float + Send + Sync>(
  ctx: &Context,
  r: &mut [NumT],
  input: &[NumT],
  periods: usize,
) -> Result<(), Error> {
  time_reg_core(
    ctx,
    r,
    input,
    periods,
    |n, sum_x, sum_x2, sum_y, sum_y2, sum_xy| {
      let numerator = n * sum_xy - sum_x * sum_y;
      let var_x = n * sum_x2 - sum_x * sum_x;
      let var_y = n * sum_y2 - sum_y * sum_y;

      let denom = var_x * var_y;
      if denom > NumT::zero() {
        // R² = (correlation)² = numerator² / (var_x * var_y)
        (numerator * numerator) / denom
      } else {
        // var_y == 0 → all y identical → indeterminate
        NumT::nan()
      }
    },
  )
}

/// Time-Series Linear Regression Residual (single-series)
///
/// Calculates the residual of the **last** observation in each rolling window
/// for an OLS regression of `y` against the implicit time index `x = [0, 1, ..., w-1]`.
///
/// ε = y_last − ŷ_last = (y_last − ȳ) − β·(x_last − x̄)
///
/// where `x_last = w - 1`, `β = (n·Σxy − Σx·Σy) / (n·Σx² − (Σx)²)`.
///
/// This requires access to the raw `y_last` value, so it is implemented
/// separately rather than through the closure-based `time_reg_core`.
pub fn ta_resi<NumT: Float + Send + Sync>(
  ctx: &Context,
  r: &mut [NumT],
  input: &[NumT],
  periods: usize,
) -> Result<(), Error> {
  if r.len() != input.len() {
    return Err(Error::LengthMismatch(r.len(), input.len()));
  }

  let r = ctx.align_end_mut(r);
  let input = ctx.align_end(input);

  if periods < 2 {
    r.fill(NumT::nan());
    return Ok(());
  }

  par_for_each_2!(r, input, ctx.chunk_size(r.len()), |r, x| {
      let start = ctx.start(r.len());
      r.fill(NumT::nan());

      if ctx.is_skip_nan() {
        let iter = SkipNanWindow::new(x, periods, start);
        let mut sum_y = NumT::zero();
        let mut sum_xy_1based = NumT::zero();
        let mut count = 0;

        for i in iter {
          for k in i.prev_start..i.start {
            let old = x[k];
            if is_normal(&old) {
              sum_xy_1based = sum_xy_1based - sum_y;
              sum_y = sum_y - old;
              count -= 1;
            }
          }

          let val = x[i.end];
          if is_normal(&val) {
            count += 1;
            let n_t = NumT::from(count).unwrap();
            sum_y = sum_y + val;
            sum_xy_1based = sum_xy_1based + n_t * val;
          }

          if !is_normal(&val) {
            continue;
          }

          let mut should_output = true;
          if ctx.is_strictly_cycle() {
            if count != periods || (i.end - i.start + 1) != periods {
              should_output = false;
            }
          }

          if should_output && count >= 2 {
            let n = NumT::from(count).unwrap();
            let sum_x = n * (n - NumT::one()) / NumT::from(2.0).unwrap();
            let sum_x2 = n * (n - NumT::one()) * (NumT::from(2.0).unwrap() * n - NumT::one())
              / NumT::from(6.0).unwrap();
            let sum_xy = sum_xy_1based - sum_y;

            let var_x = n * sum_x2 - sum_x * sum_x;

            if var_x.abs() > NumT::epsilon() {
              let beta = (n * sum_xy - sum_x * sum_y) / var_x;
              let mean_x = sum_x / n;
              let mean_y = sum_y / n;
              // x_last = n - 1 (last index in window [0..n-1])
              let x_last = n - NumT::one();
              // y_last = val (the current value at i.end)
              let y_last = val;
              r[i.end] = (y_last - mean_y) - beta * (x_last - mean_x);
            } else {
              r[i.end] = NumT::nan();
            }
          }
        }
      } else {
        let mut sum_y = NumT::zero();
        let mut sum_xy_1based = NumT::zero();
        let mut count = 0;
        let mut nan_in_window = 0;

        let pre_fill_start = if start >= periods { start - periods } else { 0 };
        for k in pre_fill_start..start {
          let val = x[k];
          if is_normal(&val) {
            count += 1;
            let n_t = NumT::from(count).unwrap();
            sum_y = sum_y + val;
            sum_xy_1based = sum_xy_1based + n_t * val;
          } else {
            count += 1;
            nan_in_window += 1;
          }
        }

        let total = r.len();
        for (idx, (r, c)) in r
          .iter_mut()
          .zip(x.iter())
          .enumerate()
          .skip(ctx.start(total))
        {
          count += 1;
          let n_t = NumT::from(count).unwrap();

          if is_normal(c) {
            sum_y = sum_y + *c;
            sum_xy_1based = sum_xy_1based + n_t * *c;
          } else {
            nan_in_window += 1;
          }

          if count > periods {
            let old_idx = idx - periods;
            let old = x[old_idx];

            sum_xy_1based = sum_xy_1based - sum_y;
            if is_normal(&old) {
              sum_y = sum_y - old;
            } else {
              nan_in_window -= 1;
            }
            count -= 1;
          }

          if count == periods {
            if nan_in_window > 0 {
              *r = NumT::nan();
            } else if ctx.is_strictly_cycle() && idx < periods - 1 {
              *r = NumT::nan();
            } else {
              let n_val = NumT::from(periods).unwrap();
              let sum_x = n_val * (n_val - NumT::one()) / NumT::from(2.0).unwrap();
              let sum_x2 =
                n_val * (n_val - NumT::one()) * (NumT::from(2.0).unwrap() * n_val - NumT::one())
                  / NumT::from(6.0).unwrap();
              let sum_xy = sum_xy_1based - sum_y;

              let var_x = n_val * sum_x2 - sum_x * sum_x;

              if var_x.abs() > NumT::epsilon() {
                let beta = (n_val * sum_xy - sum_x * sum_y) / var_x;
                let mean_x = sum_x / n_val;
                let mean_y = sum_y / n_val;
                let x_last = n_val - NumT::one();
                let y_last = *c;
                *r = (y_last - mean_y) - beta * (x_last - mean_x);
              } else {
                *r = NumT::nan();
              }
            }
          } else {
            *r = NumT::nan();
          }
        }
      }
  });
  Ok(())
}

#[cfg(test)]
mod tests {
  use super::*;
  use crate::algo::{assert_vec_eq_nan, context::FLAG_SKIP_NAN, ta_slope, ta_intercept};

  // ─── ta_rsqr tests ───────────────────────────────────────────

  #[test]
  fn test_ta_rsqr_perfect_fit() {
    // y = 2x + 1 → perfect linear → R² = 1.0
    let input = vec![1.0, 3.0, 5.0, 7.0, 9.0];
    let periods = 3;
    let mut r = vec![0.0; input.len()];
    let ctx = Context::new(0, 0, 0);
    ta_rsqr(&ctx, &mut r, &input, periods).unwrap();

    assert_vec_eq_nan(&r, &vec![f64::NAN, f64::NAN, 1.0, 1.0, 1.0]);
  }

  #[test]
  fn test_ta_rsqr_flat() {
    // All same values → var_y = 0 → R² = NaN (indeterminate)
    let input = vec![5.0, 5.0, 5.0, 5.0, 5.0];
    let periods = 3;
    let mut r = vec![0.0; input.len()];
    let ctx = Context::new(0, 0, 0);
    ta_rsqr(&ctx, &mut r, &input, periods).unwrap();

    assert_vec_eq_nan(
      &r,
      &vec![f64::NAN, f64::NAN, f64::NAN, f64::NAN, f64::NAN],
    );
  }

  #[test]
  fn test_ta_rsqr_partial() {
    // Non-perfect fit
    // Window [1, 2, 4]: x=[0,1,2], y=[1,2,4]
    // n=3, sum_x=3, sum_x2=5, sum_y=7, sum_y2=21, sum_xy=0*1+1*2+2*4=10
    // numerator = 3*10 - 3*7 = 9
    // var_x = 3*5 - 9 = 6
    // var_y = 3*21 - 49 = 14
    // R² = 81 / (6*14) = 81/84 ≈ 0.964286
    let input = vec![1.0, 2.0, 4.0];
    let periods = 3;
    let mut r = vec![0.0; input.len()];
    let ctx = Context::new(0, 0, 0);
    ta_rsqr(&ctx, &mut r, &input, periods).unwrap();

    let expected_r2 = 81.0 / 84.0;
    assert!((r[2] - expected_r2).abs() < 1e-10);
  }

  #[test]
  fn test_ta_rsqr_skip_nan() {
    let input = vec![1.0, 3.0, f64::NAN, 5.0, 7.0];
    let periods = 3;
    let mut r = vec![0.0; input.len()];
    let ctx = Context::new(0, 0, FLAG_SKIP_NAN);
    ta_rsqr(&ctx, &mut r, &input, periods).unwrap();

    // idx 1: count=2, [1,3] → x=[0,1], y=[1,3], perfect → R²=1
    // idx 2: NaN
    // idx 3: count=3, [1,3,5] skip NaN → perfect → R²=1
    // idx 4: count=3, [3,5,7] → perfect → R²=1
    let expected = vec![f64::NAN, 1.0, f64::NAN, 1.0, 1.0];
    assert_vec_eq_nan(&r, &expected);
  }

  // ─── ta_resi tests ───────────────────────────────────────────

  #[test]
  fn test_ta_resi_perfect_fit() {
    // y = 2x + 1 → perfect linear → residual = 0
    let input = vec![1.0, 3.0, 5.0, 7.0, 9.0];
    let periods = 3;
    let mut r = vec![0.0; input.len()];
    let ctx = Context::new(0, 0, 0);
    ta_resi(&ctx, &mut r, &input, periods).unwrap();

    assert_vec_eq_nan(&r, &vec![f64::NAN, f64::NAN, 0.0, 0.0, 0.0]);
  }

  #[test]
  fn test_ta_resi_with_error() {
    // Window [1, 2, 4]: x=[0,1,2], y=[1,2,4]
    // mean_x = 1, mean_y = 7/3
    // beta = (3*10 - 3*7) / (3*5 - 9) = 9/6 = 1.5
    // y_last = 4, x_last = 2
    // residual = (4 - 7/3) - 1.5 * (2 - 1) = 5/3 - 1.5 = 5/3 - 3/2 = 1/6
    let input = vec![1.0, 2.0, 4.0];
    let periods = 3;
    let mut r = vec![0.0; input.len()];
    let ctx = Context::new(0, 0, 0);
    ta_resi(&ctx, &mut r, &input, periods).unwrap();

    let expected_residual = 1.0 / 6.0;
    assert!(
      (r[2] - expected_residual).abs() < 1e-10,
      "got {} expected {}",
      r[2],
      expected_residual
    );
  }

  #[test]
  fn test_ta_resi_sliding() {
    // y = [2, 4, 6, 9]
    // Window [2,4,6]: perfect → residual = 0
    // Window [4,6,9]: x=[0,1,2], y=[4,6,9]
    //   mean_x=1, mean_y=19/3
    //   beta = (3*(0*4+1*6+2*9) - 3*19) / (3*5-9) = (3*24 - 57)/6 = 15/6 = 2.5
    //   residual = (9 - 19/3) - 2.5 * (2 - 1) = 8/3 - 2.5 = 8/3 - 5/2 = 1/6
    let input = vec![2.0, 4.0, 6.0, 9.0];
    let periods = 3;
    let mut r = vec![0.0; input.len()];
    let ctx = Context::new(0, 0, 0);
    ta_resi(&ctx, &mut r, &input, periods).unwrap();

    let expected = vec![f64::NAN, f64::NAN, 0.0, 1.0 / 6.0];
    assert_vec_eq_nan(&r, &expected);
  }

  #[test]
  fn test_ta_resi_skip_nan() {
    let input = vec![1.0, 3.0, f64::NAN, 5.0, 7.0];
    let periods = 3;
    let mut r = vec![0.0; input.len()];
    let ctx = Context::new(0, 0, FLAG_SKIP_NAN);
    ta_resi(&ctx, &mut r, &input, periods).unwrap();

    // idx 1: [1,3] → perfect → residual = 0
    // idx 3: [1,3,5] → perfect → residual = 0
    // idx 4: [3,5,7] → perfect → residual = 0
    let expected = vec![f64::NAN, 0.0, f64::NAN, 0.0, 0.0];
    assert_vec_eq_nan(&r, &expected);
  }

  #[test]
  fn test_ta_resi_consistency_with_slope_intercept() {
    // Verify: residual = y_last - (slope * x_last + intercept)
    let input = vec![1.0, 2.0, 4.0, 3.0, 7.0];
    let periods = 3;
    let ctx = Context::new(0, 0, 0);

    let mut r_resi = vec![0.0; input.len()];
    ta_resi(&ctx, &mut r_resi, &input, periods).unwrap();

    let mut r_slope = vec![0.0; input.len()];
    ta_slope(&ctx, &mut r_slope, &input, periods).unwrap();

    let mut r_intercept = vec![0.0; input.len()];
    ta_intercept(&ctx, &mut r_intercept, &input, periods).unwrap();

    // For each valid window, check: resi = y_last - (slope * (periods-1) + intercept)
    for i in (periods - 1)..input.len() {
      if r_resi[i].is_nan() {
        continue;
      }
      let x_last = (periods - 1) as f64;
      let y_hat = r_slope[i] * x_last + r_intercept[i];
      let expected_resi = input[i] - y_hat;
      assert!(
        (r_resi[i] - expected_resi).abs() < 1e-10,
        "idx {} mismatch: resi={}, expected={}",
        i,
        r_resi[i],
        expected_resi
      );
    }
  }
}
