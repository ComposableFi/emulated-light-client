/// Reads `STRESS_TEST_ITERATIONS` environment variable to determine how many
/// iterations random tests should try.
///
/// The variable is used by stress tests which generate random data to verify
/// invariant.  By default they run hundred thousand iterations.  The
/// aforementioned environment variable allows that number to be changed
/// (including to zero which effectively disables such tests).
pub(crate) fn get_iteration_count() -> usize {
    use core::str::FromStr;
    match std::env::var_os("STRESS_TEST_ITERATIONS") {
        None => 100_000,
        Some(val) => usize::from_str(val.to_str().unwrap()).unwrap(),
    }
}

/// Returns zero if dividend is zero otherwise `max(dividend / divisor, 1)`.
pub(crate) fn div_max_1(dividind: usize, divisor: usize) -> usize {
    if dividind == 0 {
        0
    } else {
        1.max(dividind / divisor)
    }
}
