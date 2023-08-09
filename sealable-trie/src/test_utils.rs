/// Reads `STRESS_TEST_ITERATIONS` environment variable to determine how many
/// iterations random tests should try.
///
/// The variable is used by stress tests which generate random data to verify
/// invariant.  By default they run ten thousand iterations.  The aforementioned
/// environment variable allows that number to be changed (including to zero
/// which effectively disables such tests).
pub(crate) fn get_iteration_count() -> usize {
    use core::str::FromStr;
    match std::env::var_os("STRESS_TEST_ITERATIONS") {
        None => 10_000,
        Some(val) => usize::from_str(val.to_str().unwrap()).unwrap(),
    }
}
