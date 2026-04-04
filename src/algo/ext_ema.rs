// Copyright 2026 MSD-RS Project LiJia
// SPDX-License-Identifier: BSD-2-Clause

use num_traits::Float;

use crate::algo::{Context, Error, is_normal};

/// SMA-seeded EMA (Industry-standard EMA)
///
/// α = 2 / (periods + 1), SMA seed from first `periods` values.
///
/// Compared to `ta_ema` (first-value seed):
///   - `ta_ema`: outputs from idx 0, seed = input[0]
///   - `ta_ema_sma_seeded`: outputs from idx N-1, seed = SMA(input[0..N])
///
/// Matches: TA-Lib / TradingView / Bloomberg EMA implementation.
///
/// Ref: https://en.wikipedia.org/wiki/Moving_average#Exponential_moving_average
pub fn ta_ema_sma_seeded<NumT: Float + Send + Sync>(
  ctx: &Context,
  r: &mut [NumT],
  input: &[NumT],
  periods: usize,
) -> Result<(), Error> {
  let alpha = NumT::from(2.0).unwrap() / NumT::from(periods + 1).unwrap();
  dma_sma_seeded_impl(ctx, r, input, alpha, periods)
}

/// Wilder's Smoothing (used by RSI, ATR-Wilder, ADX)
///
/// α = 1 / periods, SMA seed from first `periods` values.
///
/// Formula: result[i] = (1/N) * input[i] + (1 - 1/N) * result[i-1]
/// Equivalent: result[i] = (result[i-1] * (N-1) + input[i]) / N
///
/// This is the smoothing method defined by J. Welles Wilder in 1978.
/// All major platforms (TA-Lib, TradingView, Bloomberg) use this for RSI/ATR.
///
/// Ref: "New Concepts in Technical Trading Systems" by J. Welles Wilder Jr.
pub fn ta_wilder_smooth<NumT: Float + Send + Sync>(
  ctx: &Context,
  r: &mut [NumT],
  input: &[NumT],
  periods: usize,
) -> Result<(), Error> {
  let alpha = NumT::one() / NumT::from(periods).unwrap();
  dma_sma_seeded_impl(ctx, r, input, alpha, periods)
}

/// Internal: SMA-seeded DMA (generic implementation)
///
/// - Computes SMA of the first `sma_periods` values as seed
/// - Outputs NaN for indices before seed position
/// - From seed position onward: result[i] = alpha * input[i] + (1-alpha) * result[i-1]
fn dma_sma_seeded_impl<NumT: Float + Send + Sync>(
  ctx: &Context,
  r: &mut [NumT],
  input: &[NumT],
  alpha: NumT,
  sma_periods: usize,
) -> Result<(), Error> {
  if r.len() != input.len() {
    return Err(Error::LengthMismatch(r.len(), input.len()));
  }

  if sma_periods == 0 {
    return Err(Error::InvalidParameter(
      "periods must be > 0".to_string(),
    ));
  }

  let r = ctx.align_end_mut(r);
  let input = ctx.align_end(input);

  if alpha < NumT::zero() || alpha > NumT::one() {
    return Err(Error::InvalidParameter(
      "alpha must be between 0 and 1".to_string(),
    ));
  }

  let k = NumT::one() - alpha;

  par_for_each_2!(r, input, ctx.chunk_size(r.len()), |r, x| {
    let start = ctx.start(r.len());
    r.fill(NumT::nan());

    if ctx.is_skip_nan() {
      // Skip NaN mode: collect first sma_periods non-NaN values for SMA seed
      let mut sum = NumT::zero();
      let mut valid_count = 0usize;
      let mut seed_end_idx: Option<usize> = None;

      for i in start..x.len() {
        let val = x[i];
        if !is_normal(&val) {
          continue;
        }

        valid_count += 1;
        sum = sum + val;

        if valid_count == sma_periods {
          let seed = sum / NumT::from(sma_periods).unwrap();
          r[i] = seed;
          seed_end_idx = Some(i);
          break;
        }
      }

      // If we found enough values, continue with EMA recursion
      if let Some(seed_idx) = seed_end_idx {
        let mut prev = r[seed_idx];
        for i in (seed_idx + 1)..x.len() {
          let val = x[i];
          if !is_normal(&val) {
            // Skip NaN, output NaN but keep prev for next valid value
            continue;
          }
          r[i] = alpha * val + k * prev;
          prev = r[i];
        }
      }
    } else {
      // Non-skip NaN mode
      if x.len() < start + sma_periods {
        // Not enough data for SMA seed
        return;
      }

      // Check for NaN in seed window and compute SMA
      let mut sum = NumT::zero();
      let mut has_nan = false;
      for i in start..(start + sma_periods) {
        let val = x[i];
        if !is_normal(&val) {
          has_nan = true;
          break;
        }
        sum = sum + val;
      }

      if has_nan {
        // Can't compute seed — all output remains NaN
        return;
      }

      let seed_idx = start + sma_periods - 1;
      let seed = sum / NumT::from(sma_periods).unwrap();
      r[seed_idx] = seed;

      let mut prev = seed;
      for i in (seed_idx + 1)..x.len() {
        let val = x[i];
        if !is_normal(&val) {
          // In non-skip mode, NaN propagates permanently
          // (same behavior as original ta_ema)
          prev = NumT::nan();
        } else if prev.is_nan() {
          // Previous was NaN, can't recover in non-skip mode
          // r[i] remains NaN
        } else {
          r[i] = alpha * val + k * prev;
          prev = r[i];
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

  // ===== ta_ema_sma_seeded tests =====

  #[test]
  fn test_ema_sma_seeded_basic() {
    // periods=3, α = 2/4 = 0.5
    // SMA seed = (10+11+12)/3 = 11.0
    // idx 3: 0.5*13 + 0.5*11.0 = 12.0
    // idx 4: 0.5*14 + 0.5*12.0 = 13.0
    let input = vec![10.0, 11.0, 12.0, 13.0, 14.0];
    let mut r = vec![0.0; 5];
    let ctx = Context::new(0, 0, 0);
    ta_ema_sma_seeded(&ctx, &mut r, &input, 3).unwrap();

    assert_vec_eq_nan(
      &r,
      &vec![f64::NAN, f64::NAN, 11.0, 12.0, 13.0],
    );
  }

  #[test]
  fn test_ema_sma_seeded_vs_first_value() {
    // Verify SMA seed differs from first-value seed
    // With first-value seed (ta_ema): [10, 10.5, 11.25, 12.125, 13.0625]
    // With SMA seed: [NaN, NaN, 11, 12, 13]
    let input = vec![10.0, 11.0, 12.0, 13.0, 14.0];
    let mut r = vec![0.0; 5];
    let ctx = Context::new(0, 0, 0);
    ta_ema_sma_seeded(&ctx, &mut r, &input, 3).unwrap();

    // SMA seed gives different values than first-value seed
    assert!(r[0].is_nan()); // SMA seed outputs NaN for early indices
    assert_eq!(r[2], 11.0); // Seed = SMA(10,11,12) = 11.0
    assert_eq!(r[3], 12.0); // 0.5*13 + 0.5*11 = 12.0
  }

  #[test]
  fn test_ema_sma_seeded_period_1() {
    // period=1: SMA seed = input[0], α = 2/2 = 1.0
    // So result = input itself
    let input = vec![10.0, 20.0, 30.0];
    let mut r = vec![0.0; 3];
    let ctx = Context::new(0, 0, 0);
    ta_ema_sma_seeded(&ctx, &mut r, &input, 1).unwrap();

    assert_vec_eq_nan(&r, &vec![10.0, 20.0, 30.0]);
  }

  #[test]
  fn test_ema_sma_seeded_with_nan() {
    // NaN in seed window (non-skip): all output NaN
    let input = vec![10.0, f64::NAN, 12.0, 13.0, 14.0];
    let mut r = vec![0.0; 5];
    let ctx = Context::new(0, 0, 0);
    ta_ema_sma_seeded(&ctx, &mut r, &input, 3).unwrap();
    assert_vec_eq_nan(&r, &vec![f64::NAN; 5]);
  }

  #[test]
  fn test_ema_sma_seeded_nan_after_seed() {
    // NaN after seed window (non-skip): NaN propagates
    let input = vec![10.0, 11.0, 12.0, f64::NAN, 14.0];
    let mut r = vec![0.0; 5];
    let ctx = Context::new(0, 0, 0);
    ta_ema_sma_seeded(&ctx, &mut r, &input, 3).unwrap();

    // Seed = SMA(10,11,12) = 11.0
    // idx 3: NaN → propagates
    // idx 4: prev is NaN → NaN
    assert_vec_eq_nan(
      &r,
      &vec![f64::NAN, f64::NAN, 11.0, f64::NAN, f64::NAN],
    );
  }

  #[test]
  fn test_ema_sma_seeded_skip_nan() {
    // With skip_nan: NaN in seed window is skipped
    let input = vec![10.0, f64::NAN, 12.0, 13.0, 14.0];
    let mut r = vec![0.0; 5];
    let ctx = Context::new(0, 0, FLAG_SKIP_NAN);
    ta_ema_sma_seeded(&ctx, &mut r, &input, 3).unwrap();

    // Collect first 3 non-NaN: 10, 12, 13. SMA seed = 35/3 ≈ 11.6667 at idx 3
    // idx 4: 0.5*14 + 0.5*11.6667 = 12.8333
    let seed = 35.0 / 3.0;
    let expected_4 = 0.5 * 14.0 + 0.5 * seed;
    assert_vec_eq_nan(
      &r,
      &vec![f64::NAN, f64::NAN, f64::NAN, seed, expected_4],
    );
  }

  #[test]
  fn test_ema_sma_seeded_skip_nan_after_seed() {
    // NaN after seed with skip_nan: skipped, prev carries forward
    let input = vec![10.0, 11.0, 12.0, f64::NAN, 14.0];
    let mut r = vec![0.0; 5];
    let ctx = Context::new(0, 0, FLAG_SKIP_NAN);
    ta_ema_sma_seeded(&ctx, &mut r, &input, 3).unwrap();

    // Seed = SMA(10,11,12) = 11.0 at idx 2
    // idx 3: NaN → skipped, r[3] = NaN
    // idx 4: 0.5*14 + 0.5*11.0 = 12.5 (prev still 11.0)
    assert_vec_eq_nan(
      &r,
      &vec![f64::NAN, f64::NAN, 11.0, f64::NAN, 12.5],
    );
  }

  #[test]
  fn test_ema_sma_seeded_constant() {
    // Constant input → EMA = constant
    let input = vec![5.0, 5.0, 5.0, 5.0, 5.0];
    let mut r = vec![0.0; 5];
    let ctx = Context::new(0, 0, 0);
    ta_ema_sma_seeded(&ctx, &mut r, &input, 3).unwrap();
    assert_vec_eq_nan(&r, &vec![f64::NAN, f64::NAN, 5.0, 5.0, 5.0]);
  }

  // ===== ta_wilder_smooth tests =====

  #[test]
  fn test_wilder_smooth_basic() {
    // periods=3, α = 1/3
    // SMA seed = (10+11+12)/3 = 11.0
    // idx 3: (11.0*2 + 13)/3 = (22+13)/3 = 35/3 = 11.6667
    // idx 4: (11.6667*2 + 14)/3 = (23.3333+14)/3 = 37.3333/3 = 12.4444
    let input = vec![10.0, 11.0, 12.0, 13.0, 14.0];
    let mut r = vec![0.0; 5];
    let ctx = Context::new(0, 0, 0);
    ta_wilder_smooth(&ctx, &mut r, &input, 3).unwrap();

    let seed = 11.0;
    let alpha = 1.0 / 3.0;
    let k = 1.0 - alpha;
    let idx3 = alpha * 13.0 + k * seed;
    let idx4 = alpha * 14.0 + k * idx3;

    assert_vec_eq_nan(&r, &vec![f64::NAN, f64::NAN, seed, idx3, idx4]);
  }

  #[test]
  fn test_wilder_smooth_vs_ema_sma_seeded() {
    // Wilder (α=1/N) is slower/smoother than EMA (α=2/(N+1))
    let input = vec![10.0, 11.0, 12.0, 13.0, 14.0];
    let mut r_wilder = vec![0.0; 5];
    let mut r_ema = vec![0.0; 5];
    let ctx = Context::new(0, 0, 0);

    ta_wilder_smooth(&ctx, &mut r_wilder, &input, 3).unwrap();
    ta_ema_sma_seeded(&ctx, &mut r_ema, &input, 3).unwrap();

    // Same seed (SMA)
    assert_eq!(r_wilder[2], r_ema[2]);

    // Wilder reacts slower → closer to seed after rising inputs
    assert!(r_wilder[3] < r_ema[3], "wilder should be slower");
    assert!(r_wilder[4] < r_ema[4], "wilder should be slower");
  }

  #[test]
  fn test_wilder_smooth_rsi_simulation() {
    // Simulate RSI calculation workflow
    // Prices: [100, 102, 101, 105, 103, 106, 104] (N=3)
    // Diffs: [NaN, +2, -1, +4, -2, +3, -2]
    // Gains: [NaN, 2, 0, 4, 0, 3, 0]
    // Losses: [NaN, 0, 1, 0, 2, 0, 2]

    let gains = vec![f64::NAN, 2.0, 0.0, 4.0, 0.0, 3.0, 0.0];
    let losses = vec![f64::NAN, 0.0, 1.0, 0.0, 2.0, 0.0, 2.0];
    let periods = 3;

    let mut avg_gains = vec![0.0; 7];
    let mut avg_losses = vec![0.0; 7];
    let ctx = Context::new(0, 0, FLAG_SKIP_NAN);

    ta_wilder_smooth(&ctx, &mut avg_gains, &gains, periods).unwrap();
    ta_wilder_smooth(&ctx, &mut avg_losses, &losses, periods).unwrap();

    // avg_gains seed: SMA(2, 0, 4) = 2.0 at idx 3
    // avg_gains[4]: (1/3)*0 + (2/3)*2.0 = 1.3333
    // avg_gains[5]: (1/3)*3 + (2/3)*1.3333 = 1 + 0.8889 = 1.8889
    // avg_gains[6]: (1/3)*0 + (2/3)*1.8889 = 1.2593

    let alpha = 1.0 / 3.0;
    let k = 1.0 - alpha;
    let ag_3 = 2.0; // SMA(2,0,4)
    let ag_4 = alpha * 0.0 + k * ag_3;
    let ag_5 = alpha * 3.0 + k * ag_4;
    let ag_6 = alpha * 0.0 + k * ag_5;

    assert!((avg_gains[3] - ag_3).abs() < 1e-6);
    assert!((avg_gains[4] - ag_4).abs() < 1e-6);
    assert!((avg_gains[5] - ag_5).abs() < 1e-6);
    assert!((avg_gains[6] - ag_6).abs() < 1e-6);

    // Compute RSI at last index
    if avg_losses[6] > 0.0 {
      let rs = avg_gains[6] / avg_losses[6];
      let rsi = 100.0 - 100.0 / (1.0 + rs);
      assert!(rsi > 0.0 && rsi < 100.0, "RSI should be between 0-100: {}", rsi);
    }
  }

  #[test]
  fn test_wilder_smooth_constant() {
    let input = vec![5.0, 5.0, 5.0, 5.0, 5.0];
    let mut r = vec![0.0; 5];
    let ctx = Context::new(0, 0, 0);
    ta_wilder_smooth(&ctx, &mut r, &input, 3).unwrap();
    assert_vec_eq_nan(&r, &vec![f64::NAN, f64::NAN, 5.0, 5.0, 5.0]);
  }

  #[test]
  fn test_wilder_smooth_period_14() {
    // RSI typical period = 14
    // Verify it doesn't panic and produces reasonable output
    let input: Vec<f64> = (0..30).map(|i| 100.0 + (i as f64) * 0.5).collect();
    let mut r = vec![0.0; 30];
    let ctx = Context::new(0, 0, 0);
    ta_wilder_smooth(&ctx, &mut r, &input, 14).unwrap();

    // First 13 should be NaN
    for i in 0..13 {
      assert!(r[i].is_nan(), "idx {} should be NaN", i);
    }
    // idx 13 should be SMA seed
    assert!(!r[13].is_nan());
    // Subsequent should be valid and increasing
    for i in 14..30 {
      assert!(!r[i].is_nan(), "idx {} should not be NaN", i);
      assert!(r[i] > r[i - 1], "should be increasing at idx {}", i);
    }
  }

  #[test]
  fn test_invalid_params() {
    let input = vec![1.0, 2.0, 3.0];
    let mut r = vec![0.0; 3];
    let ctx = Context::default();

    assert!(ta_ema_sma_seeded(&ctx, &mut r, &input, 0).is_err());
    assert!(ta_wilder_smooth(&ctx, &mut r, &input, 0).is_err());
  }
}
