// Copyright 2026 MSD-RS Project LiJia
// SPDX-License-Identifier: BSD-2-Clause

use num_traits::Float;

use crate::algo::{Context, Error, is_normal, skip_nan_window::SkipNanWindow};

/// Rolling Quantile (Percentile)
///
/// Calculates the quantile value over a rolling window of `periods` elements.
/// `quantile` should be in range [0.0, 1.0], where 0.0 is the minimum and
/// 1.0 is the maximum.
///
/// Uses linear interpolation between adjacent ranks (same as numpy/pandas default).
///
/// This operator is used to compute Alpha158's QTLU/QTLD factors:
///   QTLU = Quantile(close, N, 0.8)
///   QTLD = Quantile(close, N, 0.2)
///
pub fn ta_quantile<NumT: Float + Send + Sync>(
  ctx: &Context,
  r: &mut [NumT],
  input: &[NumT],
  periods: usize,
  quantile: NumT,
) -> Result<(), Error> {
  if r.len() != input.len() {
    return Err(Error::LengthMismatch(r.len(), input.len()));
  }

  if quantile < NumT::zero() || quantile > NumT::one() {
    return Err(Error::InvalidParameter(
      "quantile must be between 0.0 and 1.0".to_string(),
    ));
  }

  if periods == 0 {
    return Err(Error::InvalidParameter(
      "periods must be > 0".to_string(),
    ));
  }

  let r = ctx.align_end_mut(r);
  let input = ctx.align_end(input);

  if periods == 1 {
    r.copy_from_slice(input);
    return Ok(());
  }

  par_for_each_2!(r, input, ctx.chunk_size(r.len()), |r, x| {
    let start = ctx.start(r.len());
    r.fill(NumT::nan());

    if ctx.is_skip_nan() {
      let iter = SkipNanWindow::new(x, periods, start);
      let mut window: Vec<NumT> = Vec::with_capacity(periods);

      for i in iter {
        // Rebuild valid window values
        window.clear();
        for k in i.start..=i.end {
          let val = x[k];
          if is_normal(&val) {
            window.push(val);
          }
        }

        if window.is_empty() || !is_normal(&x[i.end]) {
          continue;
        }

        if ctx.is_strictly_cycle() {
          if i.no_nan_count != periods || (i.end - i.start + 1) != periods {
            continue;
          }
        }

        r[i.end] = compute_quantile(&mut window, quantile);
      }
    } else {
      let mut window: Vec<NumT> = Vec::with_capacity(periods);

      for i in start..x.len() {
        if i + 1 >= periods {
          // Full window: [i-periods+1 .. i]
          window.clear();
          let mut has_nan = false;
          for k in (i + 1 - periods)..=i {
            if is_normal(&x[k]) {
              window.push(x[k]);
            } else {
              has_nan = true;
              break;
            }
          }

          if !has_nan && window.len() == periods {
            r[i] = compute_quantile(&mut window, quantile);
          }
        } else if !ctx.is_strictly_cycle() {
          // Partial window: [0 .. i]
          window.clear();
          let mut has_nan = false;
          for k in 0..=i {
            if is_normal(&x[k]) {
              window.push(x[k]);
            } else {
              has_nan = true;
              break;
            }
          }

          if !has_nan && !window.is_empty() {
            r[i] = compute_quantile(&mut window, quantile);
          }
        }
        // else: strictly_cycle and not enough data → remains NaN
      }
    }
  });

  Ok(())
}

/// Compute the quantile from a mutable slice using linear interpolation.
///
/// This matches numpy's default `interpolation='linear'` / pandas `.quantile()`.
/// The slice is sorted in-place.
fn compute_quantile<NumT: Float>(data: &mut [NumT], quantile: NumT) -> NumT {
  let n = data.len();
  if n == 0 {
    return NumT::nan();
  }
  if n == 1 {
    return data[0];
  }

  // Sort
  data.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));

  // Linear interpolation (numpy default)
  // virtual_index = quantile * (n - 1)
  let idx_f = quantile * NumT::from(n - 1).unwrap();
  let lo = idx_f.floor();
  let hi = idx_f.ceil();
  let frac = idx_f - lo;

  let lo_idx = lo.to_usize().unwrap_or(0).min(n - 1);
  let hi_idx = hi.to_usize().unwrap_or(0).min(n - 1);

  if lo_idx == hi_idx {
    data[lo_idx]
  } else {
    data[lo_idx] * (NumT::one() - frac) + data[hi_idx] * frac
  }
}

#[cfg(test)]
mod tests {
  use super::*;
  use crate::algo::{assert_vec_eq_nan, context::FLAG_SKIP_NAN};

  #[test]
  fn test_quantile_median() {
    // Median (quantile=0.5) of [1, 2, 3, 4, 5] with window=3
    let input = vec![1.0, 2.0, 3.0, 4.0, 5.0];
    let mut r = vec![0.0; 5];
    let ctx = Context::new(0, 0, 0);
    ta_quantile(&ctx, &mut r, &input, 3, 0.5).unwrap();
    // Window [1,2,3] → median=2.0
    // Window [2,3,4] → median=3.0
    // Window [3,4,5] → median=4.0
    // First two are partial: [1]→1.0, [1,2]→1.5
    let expected = vec![1.0, 1.5, 2.0, 3.0, 4.0];
    assert_vec_eq_nan(&r, &expected);
  }

  #[test]
  fn test_quantile_strictly_cycle() {
    let input = vec![1.0, 2.0, 3.0, 4.0, 5.0];
    let mut r = vec![0.0; 5];
    let ctx = Context::new(0, 0, crate::algo::context::FLAG_STRICTLY_CYCLE);
    ta_quantile(&ctx, &mut r, &input, 3, 0.5).unwrap();
    let expected = vec![f64::NAN, f64::NAN, 2.0, 3.0, 4.0];
    assert_vec_eq_nan(&r, &expected);
  }

  #[test]
  fn test_quantile_upper() {
    // 80th percentile (QTLU)
    let input = vec![1.0, 2.0, 3.0, 4.0, 5.0];
    let mut r = vec![0.0; 5];
    let ctx = Context::new(0, 0, crate::algo::context::FLAG_STRICTLY_CYCLE);
    ta_quantile(&ctx, &mut r, &input, 5, 0.8).unwrap();
    // Window [1,2,3,4,5]. quantile(0.8) = 0.8 * 4 = 3.2 → data[3] * 0.2 + data[4] * 0.8 = err
    // Actually: idx_f = 0.8 * 4 = 3.2. lo=3, hi=4, frac=0.2. 
    // sorted: [1,2,3,4,5]. data[3]=4, data[4]=5. result = 4*0.8 + 5*0.2 = 3.2 + 1.0 = 4.2
    let expected = vec![f64::NAN, f64::NAN, f64::NAN, f64::NAN, 4.2];
    assert_vec_eq_nan(&r, &expected);
  }

  #[test]
  fn test_quantile_lower() {
    // 20th percentile (QTLD)
    let input = vec![1.0, 2.0, 3.0, 4.0, 5.0];
    let mut r = vec![0.0; 5];
    let ctx = Context::new(0, 0, crate::algo::context::FLAG_STRICTLY_CYCLE);
    ta_quantile(&ctx, &mut r, &input, 5, 0.2).unwrap();
    // idx_f = 0.2 * 4 = 0.8. lo=0, hi=1, frac=0.8.
    // sorted: [1,2,3,4,5]. data[0]=1, data[1]=2. result = 1*0.2 + 2*0.8 = 0.2 + 1.6 = 1.8
    let expected = vec![f64::NAN, f64::NAN, f64::NAN, f64::NAN, 1.8];
    assert_vec_eq_nan(&r, &expected);
  }

  #[test]
  fn test_quantile_with_nan() {
    let input = vec![1.0, f64::NAN, 3.0, 4.0, 5.0];
    let mut r = vec![0.0; 5];
    let ctx = Context::new(0, 0, 0);
    ta_quantile(&ctx, &mut r, &input, 3, 0.5).unwrap();
    // No skip_nan: any NaN in window → NaN
    // [1]→1.0, [1,NaN]→NaN, [1,NaN,3]→NaN, [NaN,3,4]→NaN, [3,4,5]→4.0
    let expected = vec![1.0, f64::NAN, f64::NAN, f64::NAN, 4.0];
    assert_vec_eq_nan(&r, &expected);
  }

  #[test]
  fn test_quantile_skip_nan() {
    let input = vec![1.0, f64::NAN, 3.0, 4.0, 5.0];
    let mut r = vec![0.0; 5];
    let ctx = Context::new(0, 0, FLAG_SKIP_NAN);
    ta_quantile(&ctx, &mut r, &input, 3, 0.5).unwrap();
    // Skip NaN: valid values in window
    // 0: [1] → 1.0
    // 1: NaN → skip
    // 2: [1, 3] → median=2.0
    // 3: [1, 3, 4] → median=3.0
    // 4: [3, 4, 5] → median=4.0
    let expected = vec![1.0, f64::NAN, 2.0, 3.0, 4.0];
    assert_vec_eq_nan(&r, &expected);
  }

  #[test]
  fn test_quantile_min_max() {
    let input = vec![5.0, 1.0, 3.0, 2.0, 4.0];
    let mut r = vec![0.0; 5];
    let ctx = Context::new(0, 0, crate::algo::context::FLAG_STRICTLY_CYCLE);

    // quantile=0.0 → min
    ta_quantile(&ctx, &mut r, &input, 3, 0.0).unwrap();
    // [5,1,3]→sorted[1,3,5]→min=1; [1,3,2]→sorted[1,2,3]→min=1; [3,2,4]→sorted[2,3,4]→min=2
    let expected_min = vec![f64::NAN, f64::NAN, 1.0, 1.0, 2.0];
    assert_vec_eq_nan(&r, &expected_min);

    // quantile=1.0 → max
    ta_quantile(&ctx, &mut r, &input, 3, 1.0).unwrap();
    let expected_max = vec![f64::NAN, f64::NAN, 5.0, 3.0, 4.0];
    assert_vec_eq_nan(&r, &expected_max);
  }

  #[test]
  fn test_quantile_invalid_params() {
    let input = vec![1.0, 2.0, 3.0];
    let mut r = vec![0.0; 3];
    let ctx = Context::default();

    assert!(ta_quantile(&ctx, &mut r, &input, 3, -0.1).is_err());
    assert!(ta_quantile(&ctx, &mut r, &input, 3, 1.1).is_err());
    assert!(ta_quantile(&ctx, &mut r, &input, 0, 0.5).is_err());
  }
}
