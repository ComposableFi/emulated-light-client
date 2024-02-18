/// Access to the account data underlying the trie.
pub trait DataRef {
    /// Returns size of the referenced data in bytes.
    fn len(&self) -> usize;

    /// Returns whether the data is empty.
    fn is_empty(&self) -> bool { self.len() == 0 }

    /// Returns a shared reference to a byte or subslice depending on the type
    /// of index.
    ///
    /// Returns `None` if index is out of bounds.
    fn get<I: core::slice::SliceIndex<[u8]>>(
        &self,
        index: I,
    ) -> Option<&I::Output>;

    /// Returns a shared reference to a byte or subslice depending on the type
    /// of index.
    ///
    /// Returns `None` if index is out of bounds.
    fn get_mut<I: core::slice::SliceIndex<[u8]>>(
        &mut self,
        index: I,
    ) -> Option<&mut I::Output>;
}

impl DataRef for [u8] {
    #[inline]
    fn len(&self) -> usize { (*self).len() }

    fn get<I: core::slice::SliceIndex<[u8]>>(
        &self,
        index: I,
    ) -> Option<&I::Output> {
        self.get(index)
    }

    fn get_mut<I: core::slice::SliceIndex<[u8]>>(
        &mut self,
        index: I,
    ) -> Option<&mut I::Output> {
        self.get_mut(index)
    }
}

impl<const N: usize> DataRef for [u8; N] {
    #[inline]
    fn len(&self) -> usize { N }

    fn get<I: core::slice::SliceIndex<[u8]>>(
        &self,
        index: I,
    ) -> Option<&I::Output> {
        self[..].get(index)
    }

    fn get_mut<I: core::slice::SliceIndex<[u8]>>(
        &mut self,
        index: I,
    ) -> Option<&mut I::Output> {
        self[..].get_mut(index)
    }
}

impl DataRef for Vec<u8> {
    #[inline]
    fn len(&self) -> usize { (**self).len() }

    fn get<I: core::slice::SliceIndex<[u8]>>(
        &self,
        index: I,
    ) -> Option<&I::Output> {
        (**self).get(index)
    }

    fn get_mut<I: core::slice::SliceIndex<[u8]>>(
        &mut self,
        index: I,
    ) -> Option<&mut I::Output> {
        (**self).get_mut(index)
    }
}

impl<D: DataRef + ?Sized> DataRef for &'_ mut D {
    fn len(&self) -> usize { (**self).len() }

    fn get<I: core::slice::SliceIndex<[u8]>>(
        &self,
        index: I,
    ) -> Option<&I::Output> {
        (**self).get(index)
    }

    fn get_mut<I: core::slice::SliceIndex<[u8]>>(
        &mut self,
        index: I,
    ) -> Option<&mut I::Output> {
        (**self).get_mut(index)
    }
}

impl<D: DataRef + ?Sized> DataRef for core::cell::RefMut<'_, D> {
    #[inline]
    fn len(&self) -> usize { (**self).len() }

    fn get<I: core::slice::SliceIndex<[u8]>>(
        &self,
        index: I,
    ) -> Option<&I::Output> {
        (**self).get(index)
    }

    fn get_mut<I: core::slice::SliceIndex<[u8]>>(
        &mut self,
        index: I,
    ) -> Option<&mut I::Output> {
        (**self).get_mut(index)
    }
}
