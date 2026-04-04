// Copyright 2026 MSD-RS Project LiJia
// SPDX-License-Identifier: BSD-2-Clause

use num_traits::Float;

use crate::algo::{Context, Error, is_normal, skip_nan_window::SkipNanWindow};

/// Population Standard Deviation (ddof=0) over a moving window
///
/// Uses divisor N (Population StdDev), which is the industry standard for
/// financial technical analysis (TA-Lib, TradingView, Bloomberg all use ddof=0).
///
/// This differs from `ta_stddev` which uses divisor N-1 (Sample StdDev, ddof=1).
///
/// Formula: σ = sqrt( Σ(xᵢ - x̄)² / N )
///
/// Ref: https://en.wikipedia.org/wiki/Standard_deviation#Population_standard_deviation
pub fn ta_stddev_pop<NumT: Float + Send + Sync>(
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

  par_for_each_2!(r, input, ctx.chunk_size(r.len()), |r, x| {
      let start = ctx.start(r.len());
      r.fill(NumT::nan());

      if ctx.is_skip_nan() {
        let iter = SkipNanWindow::new(x, periods, start);
        let mut sum = NumT::zero();
        let mut sum_sq = NumT::zero();

        for i in iter {
          let val = x[i.end];
          if is_normal(&val) {
            sum = sum + val;
            sum_sq = sum_sq + val * val;
          }

          for k in i.prev_start..i.start {
            let old = x[k];
            if is_normal(&old) {
              sum = sum - old;
              sum_sq = sum_sq - old * old;
            }
          }

          if !is_normal(&val) {
            continue;
          }

          // Calculate Population StdDev (ddof=0)
          let count = NumT::from(i.no_nan_count).unwrap();

          let mut should_output = true;
          if ctx.is_strictly_cycle() {
            if i.no_nan_count != periods || (i.end - i.start + 1) != periods {
              should_output = false;
            }
          }

          if should_output {
            if i.no_nan_count >= 2 {
              // Variance = (SumSq - (Sum^2)/N) / N  (Population, ddof=0)
              let var_num = sum_sq - (sum * sum / count);
              let var = var_num / count;
              if var < NumT::zero() {
                r[i.end] = NumT::zero();
              } else {
                r[i.end] = var.sqrt();
              }
            } else {
              // Population stddev with N=1: σ = 0
              r[i.end] = NumT::zero();
            }
          }
        }
      } else {
        let mut sum = NumT::zero();
        let mut sum_sq = NumT::zero();
        let mut nan_in_window = 0;

        let pre_fill_start = if start >= periods { start - periods } else { 0 };

        for k in pre_fill_start..start {
          let val = x[k];
          if is_normal(&val) {
            sum = sum + val;
            sum_sq = sum_sq + val * val;
          } else {
            nan_in_window += 1;
          }
        }

        for i in start..x.len() {
          let val = x[i];

          if is_normal(&val) {
            sum = sum + val;
            sum_sq = sum_sq + val * val;
          } else {
            nan_in_window += 1;
          }

          if i >= periods {
            let old = x[i - periods];
            if is_normal(&old) {
              sum = sum - old;
              sum_sq = sum_sq - old * old;
            } else {
              nan_in_window -= 1;
            }
          }

          if nan_in_window > 0 || !is_normal(&val) {
            // Result NaN
          } else {
            let mut valid = false;
            if ctx.is_strictly_cycle() {
              if i >= periods - 1 {
                valid = true;
              }
            } else {
              if i >= periods - 1 {
                valid = true;
              }
            }

            if valid {
              let count = NumT::from(periods).unwrap();
              if periods >= 2 {
                // Population Variance = (SumSq - (Sum^2)/N) / N  (ddof=0)
                let var_num = sum_sq - (sum * sum / count);
                let var = var_num / count;
                if var < NumT::zero() {
                  r[i] = NumT::zero();
                } else {
                  r[i] = var.sqrt();
                }
              } else {
                // Population stddev with N=1: σ = 0
                r[i] = NumT::zero();
              }
            }
          }
        }
      }
  });

  Ok(())
}

#[cfg(test)]
mod tests {
  use super::*;
  use crate::algo::{assert_vec_eq_nan, context::FLAG_SKIP_NAN};

  #[test]
  fn test_stddev_pop_vs_sample() {
    // Compare Population (ddof=0) vs Sample (ddof=1)
    // Data [1, 2, 3], N=3, Mean=2
    // Population: σ = sqrt((1+0+1)/3) = sqrt(0.6667) = 0.81650
    // Sample:     s = sqrt((1+0+1)/2) = sqrt(1.0)    = 1.0
    let input = vec![1.0, 2.0, 3.0, 4.0, 5.0];
    let periods = 3;
    let mut r = vec![0.0; input.len()];
    let ctx = Context::new(0, 0, 0);
    ta_stddev_pop(&ctx, &mut r, &input, periods).unwrap();

    // Window [1,2,3]: Mean=2, SumSq=14. Var=(14-12)/3 = 0.6667. Std=0.8165
    // Window [2,3,4]: Mean=3, SumSq=29. Var=(29-27)/3 = 0.6667. Std=0.8165
    // Window [3,4,5]: Mean=4, SumSq=50. Var=(50-48)/3 = 0.6667. Std=0.8165
    let expected_std = (2.0f64 / 3.0).sqrt();
    assert_vec_eq_nan(
      &r,
      &vec![f64::NAN, f64::NAN, expected_std, expected_std, expected_std],
    );
  }

  #[test]
  fn test_stddev_pop_single_value() {
    // Population stddev with N=1 should be 0
    let input = vec![5.0];
    let mut r = vec![0.0; 1];
    let ctx = Context::new(0, 0, 0);
    ta_stddev_pop(&ctx, &mut r, &input, 1).unwrap();
    // periods=1, single value -> σ = 0
    // But periods=1 with i >= periods-1 means i >= 0, so valid from start
    assert_vec_eq_nan(&r, &vec![0.0]);
  }

  #[test]
  fn test_stddev_pop_constant() {
    // All same values -> stddev = 0
    let input = vec![3.0, 3.0, 3.0, 3.0];
    let mut r = vec![0.0; 4];
    let ctx = Context::new(0, 0, 0);
    ta_stddev_pop(&ctx, &mut r, &input, 3).unwrap();
    assert_vec_eq_nan(&r, &vec![f64::NAN, f64::NAN, 0.0, 0.0]);
  }

  #[test]
  fn test_stddev_pop_skip_nan() {
    let input = vec![1.0, 2.0, f64::NAN, 4.0, 5.0];
    let periods = 3;
    let mut r = vec![0.0; input.len()];

    // No skip (should be NaN when NaN in window)
    let ctx = Context::new(0, 0, 0);
    ta_stddev_pop(&ctx, &mut r, &input, periods).unwrap();
    assert_vec_eq_nan(
      &r,
      &vec![f64::NAN, f64::NAN, f64::NAN, f64::NAN, f64::NAN],
    );

    // Skip Nan
    let ctx = Context::new(0, 0, FLAG_SKIP_NAN);
    ta_stddev_pop(&ctx, &mut r, &input, periods).unwrap();
    // 0: [1] -> σ=0
    // 1: [1,2]. N=2. Pop: Var = ((1+4) - 9/2)/2 = (5-4.5)/2 = 0.25. σ=0.5
    // 2: NaN -> skip
    // 3: [1,2,4]. N=3. Pop: Mean=7/3. SumSq=1+4+16=21. Var=(21 - 49/3)/3 = (21-16.333)/3 = 1.5556. σ=1.2472
    // 4: [2,4,5]. N=3. Pop: Mean=11/3. SumSq=4+16+25=45. Var=(45 - 121/3)/3 = (45-40.333)/3 = 1.5556. σ=1.2472

    let exp_01 = 0.5f64;
    let exp_34 = (14.0f64 / 9.0).sqrt();
    let expected = vec![0.0, exp_01, f64::NAN, exp_34, exp_34];
    assert_vec_eq_nan(&r, &expected);
  }

  #[test]
  fn test_stddev_pop_matches_known_values() {
    // Verify against known TA-Lib Bollinger Band stddev values
    // Data: [10, 12, 11, 13, 14], period=5
    // Mean = 12, SumSq = 100+144+121+169+196 = 730
    // Population Var = (730 - 5*144) / 5 = (730 - 720) / 5 = 2.0
    // Population Std = sqrt(2.0) = 1.41421356...
    let input = vec![10.0, 12.0, 11.0, 13.0, 14.0];
    let mut r = vec![0.0; 5];
    let ctx = Context::new(0, 0, 0);
    ta_stddev_pop(&ctx, &mut r, &input, 5).unwrap();

    let expected_std = 2.0f64.sqrt();
    assert_vec_eq_nan(
      &r,
      &vec![f64::NAN, f64::NAN, f64::NAN, f64::NAN, expected_std],
    );
  }
}
