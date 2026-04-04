// Copyright 2026 MSD-RS Project LiJia
// SPDX-License-Identifier: BSD-2-Clause

use num_traits::Float;

use crate::algo::{Context, Error, is_normal, skip_nan_window::SkipNanWindow};

/// Rolling Rank Percentile over a moving window
///
/// Calculates the rank percentile of the current value within a rolling window
/// of `periods` elements. The result is in range [0.0, 1.0].
///
/// Formula: rank_pct[i] = count(window_values <= data[i]) / valid_count_in_window
///
/// This differs from `ta_quantile(data, N, q)` which returns the value at the
/// q-th quantile of the window (a "value lookup"). `ta_rank_pct` returns the
/// rank position of the current value (a "rank lookup").
///
/// Example:
///   input = [10, 20, 30, 15, 25], periods=3, strictly_cycle
///   idx 2: [10,20,30] → count(<=30)/3 = 3/3 = 1.000
///   idx 3: [20,30,15] → count(<=15)/3 = 1/3 = 0.333
///   idx 4: [30,15,25] → count(<=25)/3 = 2/3 = 0.667
pub fn ta_rank_pct<NumT: Float + Send + Sync>(
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

  if periods == 1 {
    // With a single-element window, current value is always the max and min.
    // rank_pct = 1/1 = 1.0
    for i in 0..r.len() {
      if is_normal(&input[i]) {
        r[i] = NumT::one();
      } else {
        r[i] = NumT::nan();
      }
    }
    return Ok(());
  }

  par_for_each_2!(r, input, ctx.chunk_size(r.len()), |r, x| {
    let start = ctx.start(r.len());
    r.fill(NumT::nan());

    if ctx.is_skip_nan() {
      let iter = SkipNanWindow::new(x, periods, start);

      for i in iter {
        let val = x[i.end];
        if !is_normal(&val) {
          continue;
        }

        if ctx.is_strictly_cycle() {
          if i.no_nan_count != periods || (i.end - i.start + 1) != periods {
            continue;
          }
        }

        // Count values <= current value in the window
        let mut le_count: usize = 0;
        let mut valid_count: usize = 0;
        for k in i.start..=i.end {
          let wval = x[k];
          if is_normal(&wval) {
            valid_count += 1;
            if wval <= val {
              le_count += 1;
            }
          }
        }

        if valid_count > 0 {
          r[i.end] = NumT::from(le_count).unwrap() / NumT::from(valid_count).unwrap();
        }
      }
    } else {
      for i in start..x.len() {
        let val = x[i];
        if !is_normal(&val) {
          continue;
        }

        if ctx.is_strictly_cycle() {
          if i + 1 < periods {
            continue;
          }
        }

        // Determine window boundaries
        let win_start = if i + 1 >= periods {
          i + 1 - periods
        } else {
          0
        };

        // Count values <= current value in the window
        let mut le_count: usize = 0;
        let mut valid_count: usize = 0;
        let mut has_nan = false;
        for k in win_start..=i {
          let wval = x[k];
          if is_normal(&wval) {
            valid_count += 1;
            if wval <= val {
              le_count += 1;
            }
          } else {
            has_nan = true;
          }
        }

        // If any NaN in window (and not skip_nan mode), result is NaN
        if has_nan {
          continue;
        }

        if valid_count > 0 {
          r[i] = NumT::from(le_count).unwrap() / NumT::from(valid_count).unwrap();
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
  fn test_rank_pct_basic() {
    let input = vec![10.0, 20.0, 30.0, 15.0, 25.0];
    let mut r = vec![0.0; 5];
    let ctx = Context::new(0, 0, FLAG_STRICTLY_CYCLE);
    ta_rank_pct(&ctx, &mut r, &input, 3).unwrap();

    // idx 0: NaN (window insufficient)
    // idx 1: NaN (window insufficient)
    // idx 2: [10,20,30] → count(<=30)/3 = 3/3 = 1.0
    // idx 3: [20,30,15] → count(<=15)/3 = 1/3 = 0.333...
    // idx 4: [30,15,25] → count(<=25)/3 = 2/3 = 0.666...
    assert_vec_eq_nan(
      &r,
      &vec![f64::NAN, f64::NAN, 1.0, 1.0 / 3.0, 2.0 / 3.0],
    );
  }

  #[test]
  fn test_rank_pct_partial_window() {
    // Without strictly_cycle, partial windows are allowed
    let input = vec![10.0, 20.0, 30.0, 15.0, 25.0];
    let mut r = vec![0.0; 5];
    let ctx = Context::new(0, 0, 0);
    ta_rank_pct(&ctx, &mut r, &input, 3).unwrap();

    // idx 0: [10] → 1/1 = 1.0
    // idx 1: [10,20] → count(<=20)/2 = 2/2 = 1.0
    // idx 2: [10,20,30] → 3/3 = 1.0
    // idx 3: [20,30,15] → count(<=15)/3 = 1/3
    // idx 4: [30,15,25] → count(<=25)/3 = 2/3
    assert_vec_eq_nan(
      &r,
      &vec![1.0, 1.0, 1.0, 1.0 / 3.0, 2.0 / 3.0],
    );
  }

  #[test]
  fn test_rank_pct_all_same() {
    // All equal values -> rank_pct = 1.0 (all values <= current)
    let input = vec![5.0, 5.0, 5.0, 5.0];
    let mut r = vec![0.0; 4];
    let ctx = Context::new(0, 0, FLAG_STRICTLY_CYCLE);
    ta_rank_pct(&ctx, &mut r, &input, 3).unwrap();
    assert_vec_eq_nan(&r, &vec![f64::NAN, f64::NAN, 1.0, 1.0]);
  }

  #[test]
  fn test_rank_pct_ascending() {
    // Strictly ascending: current value is always the max -> rank_pct = 1.0
    let input = vec![1.0, 2.0, 3.0, 4.0, 5.0];
    let mut r = vec![0.0; 5];
    let ctx = Context::new(0, 0, FLAG_STRICTLY_CYCLE);
    ta_rank_pct(&ctx, &mut r, &input, 3).unwrap();
    assert_vec_eq_nan(&r, &vec![f64::NAN, f64::NAN, 1.0, 1.0, 1.0]);
  }

  #[test]
  fn test_rank_pct_descending() {
    // Strictly descending: current value is always the min -> rank_pct = 1/N
    let input = vec![5.0, 4.0, 3.0, 2.0, 1.0];
    let mut r = vec![0.0; 5];
    let ctx = Context::new(0, 0, FLAG_STRICTLY_CYCLE);
    ta_rank_pct(&ctx, &mut r, &input, 3).unwrap();
    // idx 2: [5,4,3] → count(<=3)/3 = 1/3
    // idx 3: [4,3,2] → count(<=2)/3 = 1/3
    // idx 4: [3,2,1] → count(<=1)/3 = 1/3
    assert_vec_eq_nan(
      &r,
      &vec![f64::NAN, f64::NAN, 1.0 / 3.0, 1.0 / 3.0, 1.0 / 3.0],
    );
  }

  #[test]
  fn test_rank_pct_with_nan() {
    let input = vec![10.0, f64::NAN, 30.0, 15.0, 25.0];
    let mut r = vec![0.0; 5];

    // No skip_nan: NaN in window -> NaN result
    let ctx = Context::new(0, 0, 0);
    ta_rank_pct(&ctx, &mut r, &input, 3).unwrap();
    // idx 0: [10] → 1.0
    // idx 1: NaN → NaN
    // idx 2: [10, NaN, 30] → has_nan → NaN
    // idx 3: [NaN, 30, 15] → has_nan → NaN
    // idx 4: [30, 15, 25] → count(<=25)/3 = 2/3
    assert_vec_eq_nan(
      &r,
      &vec![1.0, f64::NAN, f64::NAN, f64::NAN, 2.0 / 3.0],
    );
  }

  #[test]
  fn test_rank_pct_skip_nan() {
    let input = vec![10.0, f64::NAN, 30.0, 15.0, 25.0];
    let mut r = vec![0.0; 5];
    let ctx = Context::new(0, 0, FLAG_SKIP_NAN);
    ta_rank_pct(&ctx, &mut r, &input, 3).unwrap();

    // idx 0: [10] → 1/1 = 1.0
    // idx 1: NaN → NaN (current is NaN)
    // idx 2: [10, 30] → count(<=30)/2 = 2/2 = 1.0
    // idx 3: [10, 30, 15] → count(<=15)/3 = 2/3
    // idx 4: [30, 15, 25] → count(<=25)/3 = 2/3
    assert_vec_eq_nan(
      &r,
      &vec![1.0, f64::NAN, 1.0, 2.0 / 3.0, 2.0 / 3.0],
    );
  }

  #[test]
  fn test_rank_pct_with_duplicates() {
    // Duplicates in window
    let input = vec![10.0, 20.0, 10.0, 10.0, 30.0];
    let mut r = vec![0.0; 5];
    let ctx = Context::new(0, 0, FLAG_STRICTLY_CYCLE);
    ta_rank_pct(&ctx, &mut r, &input, 3).unwrap();

    // idx 2: [10,20,10] → count(<=10)/3 = 2/3
    // idx 3: [20,10,10] → count(<=10)/3 = 2/3
    // idx 4: [10,10,30] → count(<=30)/3 = 3/3 = 1.0
    assert_vec_eq_nan(
      &r,
      &vec![f64::NAN, f64::NAN, 2.0 / 3.0, 2.0 / 3.0, 1.0],
    );
  }

  #[test]
  fn test_rank_pct_single_period() {
    let input = vec![1.0, 2.0, f64::NAN, 4.0];
    let mut r = vec![0.0; 4];
    let ctx = Context::new(0, 0, 0);
    ta_rank_pct(&ctx, &mut r, &input, 1).unwrap();
    assert_vec_eq_nan(&r, &vec![1.0, 1.0, f64::NAN, 1.0]);
  }

  #[test]
  fn test_rank_pct_invalid_params() {
    let input = vec![1.0, 2.0, 3.0];
    let mut r = vec![0.0; 3];
    let ctx = Context::default();

    assert!(ta_rank_pct(&ctx, &mut r, &input, 0).is_err());
  }
}
