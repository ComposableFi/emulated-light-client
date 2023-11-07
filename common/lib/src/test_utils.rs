/// Reads `STRESS_TEST_ITERATIONS` environment variable to determine how many
/// iterations stress tests should run.
///
/// The variable is used by tests which generate random data to verify
/// invariants.  By default they run hundred thousand iterations.  The
/// aforementioned environment variable allows that number to be changed
/// (including to zero which effectively disables such tests).
///
/// Heavier tests can use `divisor` argument greater than one to return the
/// value reduced by given factor.  Using `divisor` is better than dividing the
/// result because if requested number of tests is non-zero, this function will
/// always return at least one.
///
/// When running under Miri, the returned value is clamped to be at most five.
/// The idea being to avoid stress tests, which run for good several second when
/// run normally, taking minutes or hours when run through Miri.
pub fn get_iteration_count(divisor: usize) -> usize {
    use core::str::FromStr;
    let n = std::env::var_os("STRESS_TEST_ITERATIONS")
        .map(|val| usize::from_str(val.to_str().unwrap()).unwrap())
        .unwrap_or(100_000);
    let n = match n {
        0 => 0,
        n => 1.max(n / divisor),
    };
    if cfg!(miri) {
        n.min(5)
    } else {
        n
    }
}
