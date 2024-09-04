#[cfg(all(feature = "rayon", not(miri)))]
use rayon::prelude::*;

pub mod prelude {
    #[cfg(all(feature = "rayon", not(miri)))]
    pub use rayon::iter::ParallelIterator;
}

#[cfg(all(feature = "rayon", not(miri)))]
pub type Chunks<'a, T> = rayon::slice::Chunks<'a, T>;
#[cfg(any(not(feature = "rayon"), miri))]
pub type Chunks<'a, T> = core::slice::Chunks<'a, T>;

/// Splits array into `count`-element chunks.
///
/// It uses conditional compilation and either uses Rayon’s `par_chunks` method
/// (to allow parallelisation of the chunk processing) or standard `[T]::chunks`
/// method.  Specifically, if `rayon` feature is enabled and not building Miri
/// tests, Rayon is used.
///
/// Note that depending on compilation features and target the function is
/// defined as returning `rayon::slice::Chunks` or `core::slice::Chunks`. types.
///
/// # Example
///
/// ```
/// #[allow(unused_imports)]
/// use lib::par::prelude::*;
///
/// let chunks = lib::par::chunks(&[0, 1, 2, 3, 4], 3)
///     .map(|chunk| chunk.to_vec())
///     .collect::<Vec<Vec<u32>>>();
/// assert_eq!(&[vec![0, 1, 2], vec![3, 4]], chunks.as_slice());
/// ```
pub fn chunks<T: Sync>(arr: &[T], count: usize) -> Chunks<'_, T> {
    #[cfg(all(feature = "rayon", not(miri)))]
    return arr.par_chunks(count);
    #[cfg(any(not(feature = "rayon"), miri))]
    return arr.chunks(count);
}

#[test]
fn test_chunks() {
    let got = chunks(&[1u32, 2, 3, 4, 5], 3)
        .map(|chunk| (chunk.len(), chunk.iter().sum::<u32>()))
        .collect::<alloc::vec::Vec<(usize, u32)>>();
    assert_eq!(&[(3, 6), (2, 9)], got.as_slice());
}


/// Sorts elements of a slice using given comparator.
///
/// It uses conditional compilation and either uses Rayon’s
/// `par_sort_unstable_by` or standard `sort_unstable_by` method.  Specifically,
/// if `rayon` feature is enabled and not building Miri tests, Rayon is used.
///
/// # Example
///
/// ```
/// let mut arr = [5, 4, 3, 1, 2, 3];
/// lib::par::sort_unstable_by(&mut arr[..], |a, b| a.cmp(b));
/// assert_eq!(&[1, 2, 3, 3, 4, 5], &arr[..]);
/// ```
pub fn sort_unstable_by<T: Send + Sync>(
    arr: &mut [T],
    cmp: impl (Fn(&T, &T) -> core::cmp::Ordering) + Sync,
) {
    #[cfg(all(feature = "rayon", not(miri)))]
    arr.par_sort_unstable_by(cmp);
    #[cfg(any(not(feature = "rayon"), miri))]
    arr.sort_unstable_by(cmp);
}
