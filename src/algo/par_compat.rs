// Copyright 2026 MSD-RS Project LiJia
// SPDX-License-Identifier: BSD-2-Clause

//! Parallel/serial compatibility macros.
//!
//! These macros provide a unified API for parallel (rayon) and serial iteration,
//! controlled by the `parallel` feature flag.

/// Two-input parallel/serial chunk iteration (r_mut, input).
///
/// Splits `$r` and `$input` into chunks of `$chunk_size`, iterating either
/// in parallel (rayon) or serially depending on the `parallel` feature.
macro_rules! par_for_each_2 {
  ($r:expr, $input:expr, $chunk_size:expr, |$r_name:ident, $x_name:ident| $body:block) => {{
    let _cs = $chunk_size;
    #[cfg(feature = "parallel")]
    {
      use rayon::prelude::*;
      $r.par_chunks_mut(_cs)
        .zip($input.par_chunks(_cs))
        .for_each(|($r_name, $x_name)| $body);
    }
    #[cfg(not(feature = "parallel"))]
    {
      $r.chunks_mut(_cs)
        .zip($input.chunks(_cs))
        .for_each(|($r_name, $x_name)| $body);
    }
  }};
}

/// Three-input parallel/serial chunk iteration (r_mut, a, b).
///
/// Splits `$r`, `$a`, `$b` into chunks of `$chunk_size`, iterating either
/// in parallel (rayon) or serially depending on the `parallel` feature.
macro_rules! par_for_each_3 {
  (
    $r:expr, $a:expr, $b:expr, $chunk_size:expr,
    |($r_name:ident, $a_name:ident), $b_name:ident| $body:block
  ) => {{
    let _cs = $chunk_size;
    #[cfg(feature = "parallel")]
    {
      use rayon::prelude::*;
      $r.par_chunks_mut(_cs)
        .zip($a.par_chunks(_cs))
        .zip($b.par_chunks(_cs))
        .for_each(|(($r_name, $a_name), $b_name)| $body);
    }
    #[cfg(not(feature = "parallel"))]
    {
      $r.chunks_mut(_cs)
        .zip($a.chunks(_cs))
        .zip($b.chunks(_cs))
        .for_each(|(($r_name, $a_name), $b_name)| $body);
    }
  }};
}

/// Four-input parallel/serial chunk iteration (r_mut, a, b, c).
macro_rules! par_for_each_4 {
  (
    $r:expr, $a:expr, $b:expr, $c:expr, $chunk_size:expr,
    |(($r_name:ident, $a_name:ident), $b_name:ident), $c_name:ident| $body:block
  ) => {{
    let _cs = $chunk_size;
    #[cfg(feature = "parallel")]
    {
      use rayon::prelude::*;
      $r.par_chunks_mut(_cs)
        .zip($a.par_chunks(_cs))
        .zip($b.par_chunks(_cs))
        .zip($c.par_chunks(_cs))
        .for_each(|((($r_name, $a_name), $b_name), $c_name)| $body);
    }
    #[cfg(not(feature = "parallel"))]
    {
      $r.chunks_mut(_cs)
        .zip($a.chunks(_cs))
        .zip($b.chunks(_cs))
        .zip($c.chunks(_cs))
        .for_each(|((($r_name, $a_name), $b_name), $c_name)| $body);
    }
  }};
}

/// Parallel/serial range iteration.
///
/// Iterates over `$range` either with `into_par_iter().for_each()` (rayon)
/// or a plain `for` loop, depending on the `parallel` feature.
macro_rules! par_range_for_each {
  ($range:expr, |$j:ident| $body:block) => {{
    #[cfg(feature = "parallel")]
    {
      use rayon::prelude::*;
      $range.into_par_iter().for_each(|$j| $body);
    }
    #[cfg(not(feature = "parallel"))]
    {
      // Wrap body in a closure to preserve `return` semantics (early exit from
      // the iteration body, not from the enclosing function).
      let _body_fn = |$j: usize| $body;
      for _j in $range {
        _body_fn(_j);
      }
    }
  }};
}
