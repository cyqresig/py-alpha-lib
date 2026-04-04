// Copyright 2026 MSD-RS Project LiJia
// SPDX-License-Identifier: BSD-2-Clause

use num_traits::Float;

use crate::algo::{Context, Error, is_normal, skip_nan_window::SkipNanWindow};

/// Rolling Z-Score using Population StdDev (ddof=0) over a moving window
///
/// Z-Score = (x - mean) / σ, where σ is the Population Standard Deviation (ddof=0).
///
/// This differs from `ta_zscore` which uses Sample StdDev (ddof=1).
/// Population StdDev is the industry standard for financial technical analysis
/// (TA-Lib, TradingView, Bloomberg).
///
/// Ref: https://en.wikipedia.org/wiki/Standard_score
pub fn ta_zscore_pop<NumT: Float + Send + Sync>(
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

          let mut should_output = true;
          if ctx.is_strictly_cycle() {
            if i.no_nan_count != periods || (i.end - i.start + 1) != periods {
              should_output = false;
            }
          }

          if should_output && i.no_nan_count > 1 {
            let count = NumT::from(i.no_nan_count).unwrap();
            let mean = sum / count;
            let var_num = sum_sq - (sum * sum / count);
            // Population variance: divide by N (ddof=0)
            let var = var_num / count;
            if var < NumT::zero() || var.abs() < NumT::epsilon() {
              r[i.end] = NumT::zero();
            } else {
              r[i.end] = (val - mean) / var.sqrt();
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
            // NaN
          } else if i >= periods - 1 && periods > 1 {
            let count = NumT::from(periods).unwrap();
            let mean = sum / count;
            let var_num = sum_sq - (sum * sum / count);
            // Population variance: divide by N (ddof=0)
            let var = var_num / count;
            if var < NumT::zero() || var.abs() < NumT::epsilon() {
              r[i] = NumT::zero();
            } else {
              r[i] = (val - mean) / var.sqrt();
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
  use crate::algo::assert_vec_eq_nan;

  #[test]
  fn test_zscore_pop_simple() {
    let input = vec![1.0, 2.0, 3.0, 4.0, 5.0];
    let periods = 3;
    let mut r = vec![0.0; input.len()];
    let ctx = Context::new(0, 0, 0);
    ta_zscore_pop(&ctx, &mut r, &input, periods).unwrap();

    // Window [1,2,3]: mean=2, pop_std=sqrt(2/3)=0.8165. zscore(3)=(3-2)/0.8165 = 1.2247
    // Window [2,3,4]: mean=3, pop_std=0.8165. zscore(4)=(4-3)/0.8165 = 1.2247
    // Window [3,4,5]: mean=4, pop_std=0.8165. zscore(5)=(5-4)/0.8165 = 1.2247
    let pop_std = (2.0f64 / 3.0).sqrt();
    let expected_z = 1.0 / pop_std;
    assert_vec_eq_nan(
      &r,
      &vec![f64::NAN, f64::NAN, expected_z, expected_z, expected_z],
    );
  }

  #[test]
  fn test_zscore_pop_vs_sample() {
    // Population z-score should be larger in absolute value than sample z-score
    // because pop_std < sample_std
    let input = vec![1.0, 2.0, 3.0];
    let periods = 3;
    let mut r_pop = vec![0.0; 3];
    let ctx = Context::new(0, 0, 0);
    ta_zscore_pop(&ctx, &mut r_pop, &input, periods).unwrap();

    // pop_std = sqrt(2/3) ≈ 0.8165
    // sample_std = sqrt(1) = 1.0
    // pop_zscore(3) = 1/0.8165 ≈ 1.2247
    // sample_zscore(3) = 1/1.0 = 1.0
    let pop_std = (2.0f64 / 3.0).sqrt();
    let expected_pop_z = 1.0 / pop_std;
    assert!((r_pop[2] - expected_pop_z).abs() < 1e-6);
    assert!(r_pop[2] > 1.0); // pop zscore > sample zscore for same data
  }

  #[test]
  fn test_zscore_pop_negative() {
    let input = vec![3.0, 2.0, 1.0];
    let periods = 3;
    let mut r = vec![0.0; 3];
    let ctx = Context::new(0, 0, 0);
    ta_zscore_pop(&ctx, &mut r, &input, periods).unwrap();

    // Window [3,2,1]: mean=2, pop_std=sqrt(2/3). zscore(1)=(1-2)/sqrt(2/3) = -1.2247
    let pop_std = (2.0f64 / 3.0).sqrt();
    let expected_z = -1.0 / pop_std;
    assert_vec_eq_nan(&r, &vec![f64::NAN, f64::NAN, expected_z]);
  }

  #[test]
  fn test_zscore_pop_constant() {
    let input = vec![5.0, 5.0, 5.0];
    let periods = 3;
    let mut r = vec![0.0; 3];
    let ctx = Context::new(0, 0, 0);
    ta_zscore_pop(&ctx, &mut r, &input, periods).unwrap();

    // All same -> std=0, zscore=0
    assert_vec_eq_nan(&r, &vec![f64::NAN, f64::NAN, 0.0]);
  }

  #[test]
  fn test_zscore_pop_with_nan() {
    let input = vec![1.0, f64::NAN, 3.0, 4.0, 5.0];
    let periods = 3;
    let mut r = vec![0.0; input.len()];

    // No skip: NaN in window -> NaN result
    let ctx = Context::new(0, 0, 0);
    ta_zscore_pop(&ctx, &mut r, &input, periods).unwrap();
    // idx 0,1: insufficient window
    // idx 2: window [1, NaN, 3] -> NaN (has NaN)
    // idx 3: window [NaN, 3, 4] -> NaN (has NaN)
    // idx 4: window [3, 4, 5] -> valid! mean=4, pop_std=sqrt(2/3)
    //   zscore(5) = (5-4)/sqrt(2/3) = 1.2247...
    let pop_std = (2.0f64 / 3.0).sqrt();
    let expected_z = 1.0 / pop_std;
    assert_vec_eq_nan(
      &r,
      &vec![f64::NAN, f64::NAN, f64::NAN, f64::NAN, expected_z],
    );
  }
}
