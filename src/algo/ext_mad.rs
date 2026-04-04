// Copyright 2026 MSD-RS Project LiJia
// SPDX-License-Identifier: BSD-2-Clause

use num_traits::Float;

use crate::algo::{Context, Error, is_normal, skip_nan_window::SkipNanWindow};

/// Rolling Mean Absolute Deviation (MAD) over a moving window
///
/// MAD = mean(|x - mean|) calculated within a rolling window of `periods`.
///
/// This is different from `ta_moment(k=1)` which equals zero by definition
/// (since mean of (x - mean) always = 0). MAD uses absolute values.
///
/// Use case: CCI denominator
///   CCI = (TP - SMA(TP, N)) / (0.015 × MAD(TP, N))
///
/// Ref: https://en.wikipedia.org/wiki/Average_absolute_deviation
pub fn ta_mad<NumT: Float + Send + Sync>(
  ctx: &Context,
  r: &mut [NumT],
  input: &[NumT],
  periods: usize,
) -> Result<(), Error> {
  if r.len() != input.len() {
    return Err(Error::LengthMismatch(r.len(), input.len()));
  }

  if periods == 0 {
    return Err(Error::InvalidParameter(
      "periods must be > 0".to_string(),
    ));
  }

  let r = ctx.align_end_mut(r);
  let input = ctx.align_end(input);

  par_for_each_2!(r, input, ctx.chunk_size(r.len()), |r, x| {
    let start = ctx.start(r.len());
    r.fill(NumT::nan());

    if ctx.is_skip_nan() {
      let iter = SkipNanWindow::new(x, periods, start);
      let mut sum = NumT::zero();

      for i in iter {
        let val = x[i.end];
        if is_normal(&val) {
          sum = sum + val;
        }

        for j in i.prev_start..i.start {
          let old = x[j];
          if is_normal(&old) {
            sum = sum - old;
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

        if should_output && i.no_nan_count >= 2 {
          let count = NumT::from(i.no_nan_count).unwrap();
          let mean = sum / count;

          // Compute MAD: mean of |x - mean| over the window
          let mut mad_sum = NumT::zero();
          for j in i.start..=i.end {
            let v = x[j];
            if is_normal(&v) {
              mad_sum = mad_sum + (v - mean).abs();
            }
          }

          r[i.end] = mad_sum / count;
        }
      }
    } else {
      let mut sum = NumT::zero();
      let mut nan_in_window = 0;

      let pre_fill_start = if start >= periods { start - periods } else { 0 };

      for j in pre_fill_start..start {
        let val = x[j];
        if is_normal(&val) {
          sum = sum + val;
        } else {
          nan_in_window += 1;
        }
      }

      for i in start..x.len() {
        let val = x[i];

        if is_normal(&val) {
          sum = sum + val;
        } else {
          nan_in_window += 1;
        }

        if i >= periods {
          let old = x[i - periods];
          if is_normal(&old) {
            sum = sum - old;
          } else {
            nan_in_window -= 1;
          }
        }

        if nan_in_window > 0 || !is_normal(&val) {
          // NaN
        } else if i >= periods - 1 && periods >= 2 {
          let count = NumT::from(periods).unwrap();
          let mean = sum / count;

          let win_start = i + 1 - periods;
          let mut mad_sum = NumT::zero();
          for j in win_start..=i {
            let v = x[j];
            mad_sum = mad_sum + (v - mean).abs();
          }

          r[i] = mad_sum / count;
        }
      }
    }
  });

  Ok(())
}

#[cfg(test)]
mod tests {
  use super::*;
  use crate::algo::{
    assert_vec_eq_nan,
    context::{FLAG_SKIP_NAN, FLAG_STRICTLY_CYCLE},
  };

  #[test]
  fn test_mad_basic() {
    // data [1, 2, 3], period=3, mean=2
    // MAD = (|1-2| + |2-2| + |3-2|) / 3 = (1+0+1)/3 = 2/3
    let input = vec![1.0, 2.0, 3.0, 4.0, 5.0];
    let mut r = vec![0.0; 5];
    let ctx = Context::new(0, 0, 0);
    ta_mad(&ctx, &mut r, &input, 3).unwrap();

    let expected_mad = 2.0 / 3.0;
    assert_vec_eq_nan(
      &r,
      &vec![f64::NAN, f64::NAN, expected_mad, expected_mad, expected_mad],
    );
  }

  #[test]
  fn test_mad_constant() {
    // All same values → MAD = 0
    let input = vec![5.0, 5.0, 5.0, 5.0];
    let mut r = vec![0.0; 4];
    let ctx = Context::new(0, 0, 0);
    ta_mad(&ctx, &mut r, &input, 3).unwrap();
    assert_vec_eq_nan(&r, &vec![f64::NAN, f64::NAN, 0.0, 0.0]);
  }

  #[test]
  fn test_mad_varying() {
    // data [10, 20, 30], mean=20
    // MAD = (|10-20|+|20-20|+|30-20|)/3 = (10+0+10)/3 = 20/3
    let input = vec![10.0, 20.0, 30.0];
    let mut r = vec![0.0; 3];
    let ctx = Context::new(0, 0, 0);
    ta_mad(&ctx, &mut r, &input, 3).unwrap();

    let expected = 20.0 / 3.0;
    assert_vec_eq_nan(&r, &vec![f64::NAN, f64::NAN, expected]);
  }

  #[test]
  fn test_mad_cci_simulation() {
    // Simulate CCI: CCI = (TP - SMA(TP)) / (0.015 * MAD(TP))
    // TP values
    let tp = vec![100.0, 102.0, 98.0, 104.0, 101.0];
    let periods = 3;
    let mut mad_out = vec![0.0; 5];
    let ctx = Context::new(0, 0, 0);
    ta_mad(&ctx, &mut mad_out, &tp, periods).unwrap();

    // idx 2: window [100, 102, 98], mean=100
    // MAD = (|100-100|+|102-100|+|98-100|)/3 = (0+2+2)/3 = 4/3
    let expected_2 = 4.0 / 3.0;
    assert!((mad_out[2] - expected_2).abs() < 1e-6);

    // idx 3: window [102, 98, 104], mean=304/3 ≈ 101.333
    // MAD = (|102-101.333|+|98-101.333|+|104-101.333|)/3
    //     = (0.667 + 3.333 + 2.667)/3 = 6.667/3 ≈ 2.222
    let mean_3 = 304.0 / 3.0;
    let expected_3 =
      ((102.0 - mean_3).abs() + (98.0 - mean_3).abs() + (104.0 - mean_3).abs()) / 3.0;
    assert!((mad_out[3] - expected_3).abs() < 1e-6);
  }

  #[test]
  fn test_mad_with_nan() {
    let input = vec![1.0, f64::NAN, 3.0, 4.0, 5.0];
    let mut r = vec![0.0; 5];
    let ctx = Context::new(0, 0, 0);
    ta_mad(&ctx, &mut r, &input, 3).unwrap();

    // Non-skip: NaN in window → NaN result
    // idx 2: [1, NaN, 3] → NaN
    // idx 3: [NaN, 3, 4] → NaN
    // idx 4: [3, 4, 5] → mean=4, MAD=(1+0+1)/3 = 2/3
    let expected_4 = 2.0 / 3.0;
    assert_vec_eq_nan(
      &r,
      &vec![f64::NAN, f64::NAN, f64::NAN, f64::NAN, expected_4],
    );
  }

  #[test]
  fn test_mad_skip_nan() {
    let input = vec![1.0, f64::NAN, 3.0, 4.0, 5.0];
    let mut r = vec![0.0; 5];
    let ctx = Context::new(0, 0, FLAG_SKIP_NAN);
    ta_mad(&ctx, &mut r, &input, 3).unwrap();

    // Skip NaN:
    // idx 0: [1] count=1 < 2 → NaN
    // idx 1: NaN → skip
    // idx 2: [1, 3] count=2. mean=2. MAD=(1+1)/2 = 1.0
    // idx 3: [1, 3, 4] count=3. mean=8/3. MAD=(|1-8/3|+|3-8/3|+|4-8/3|)/3
    //        = (5/3 + 1/3 + 4/3)/3 = (10/3)/3 = 10/9
    // idx 4: [3, 4, 5] count=3. mean=4. MAD=(1+0+1)/3 = 2/3
    let mean_3 = 8.0 / 3.0;
    let expected_3 =
      ((1.0 - mean_3).abs() + (3.0 - mean_3).abs() + (4.0 - mean_3).abs()) / 3.0;
    let expected_4 = 2.0 / 3.0;
    assert_vec_eq_nan(
      &r,
      &vec![f64::NAN, f64::NAN, 1.0, expected_3, expected_4],
    );
  }

  #[test]
  fn test_mad_strictly_cycle() {
    let input = vec![1.0, 2.0, 3.0, 4.0, 5.0];
    let mut r = vec![0.0; 5];
    let ctx = Context::new(0, 0, FLAG_STRICTLY_CYCLE);
    ta_mad(&ctx, &mut r, &input, 3).unwrap();

    let expected_mad = 2.0 / 3.0;
    assert_vec_eq_nan(
      &r,
      &vec![f64::NAN, f64::NAN, expected_mad, expected_mad, expected_mad],
    );
  }

  #[test]
  fn test_mad_invalid_params() {
    let input = vec![1.0, 2.0, 3.0];
    let mut r = vec![0.0; 3];
    let ctx = Context::default();

    assert!(ta_mad(&ctx, &mut r, &input, 0).is_err());
  }
}
