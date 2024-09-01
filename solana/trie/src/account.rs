use core::slice::SliceIndex;

use solana_program::account_info::AccountInfo;
use solana_program::entrypoint::MAX_PERMITTED_DATA_INCREASE;
use solana_program::program_error::ProgramError;
use solana_program::rent::Rent;
use solana_program::system_instruction::MAX_PERMITTED_DATA_LENGTH;
use solana_program::sysvar::Sysvar;

/// An account backing a Trie which can be resized.
#[derive(Debug)]
pub struct ResizableAccount<'a, 'info> {
    account: &'a AccountInfo<'info>,
    payer: &'a AccountInfo<'info>,
}

impl<'a, 'info> ResizableAccount<'a, 'info> {
    /// Grab’s account’s data and constructs new object.
    pub(crate) fn new(
        account: &'a AccountInfo<'info>,
        payer: &'a AccountInfo<'info>,
    ) -> Result<Self, ProgramError> {
        Ok(Self { account, payer })
    }

    /// Makes sure account has enough lamports to be rent-exempt if it’s size is
    /// `new_len`.
    ///
    /// If account’s balance is to low, transfers lamports from the payer.
    #[must_use]
    fn ensure_minimum_balance(&mut self, new_len: usize) -> bool {
        let minimum = match Rent::get() {
            Ok(rent) => rent.minimum_balance(new_len),
            Err(_) => return false,
        };
        let lamports = minimum.saturating_sub(self.account.lamports());
        if lamports == 0 {
            return true;
        }
        let ix = solana_program::system_instruction::transfer(
            self.payer.key,
            self.account.key,
            lamports,
        );
        let accounts = [self.payer.clone(), self.account.clone()];
        solana_program::program::invoke(&ix, &accounts).is_ok()
    }
}

impl<'a, 'info> crate::data_ref::DataRef for ResizableAccount<'a, 'info> {
    #[inline]
    fn len(&self) -> usize { self.get(..).map_or(0, |bytes| bytes.len()) }

    #[inline]
    fn is_empty(&self) -> bool { self.len() == 0 }

    fn get<I: SliceIndex<[u8]>>(&self, index: I) -> Option<&I::Output> {
        unsafe fn transmute_lifetime<'a, T: ?Sized>(arg: &T) -> &'a T {
            unsafe { core::mem::transmute(arg) }
        }

        let guard = self.account.try_borrow_data().unwrap();
        let ret = guard.get(index);
        // SAFETY: Transmute the lifetime so we don’t need to hold to the guard.
        // The data pointed by AccountInfo outlives the AccountInfo and never
        // moves.  The size may change during resizing but even then the
        // entirety of slice is valid.
        ret.map(|ret| unsafe { transmute_lifetime(ret) })
    }

    fn get_mut<I: SliceIndex<[u8]>>(
        &mut self,
        index: I,
    ) -> Option<&mut I::Output> {
        unsafe fn transmute_lifetime<'a, T: ?Sized>(arg: &mut T) -> &'a mut T {
            unsafe { core::mem::transmute(arg) }
        }

        let mut guard = self.account.try_borrow_mut_data().unwrap();
        let ret = guard.get_mut(index);
        // SAFETY: See comment in Self::get.
        ret.map(|ret| unsafe { transmute_lifetime(ret) })
    }

    /// Enlarge the account to hold at least `min_size` bytes.
    ///
    /// Always enlarges to the maximum allowable size, i.e. by 10 KiB from
    /// account’s initial size.
    fn enlarge(&mut self, min_size: usize) -> bool {
        if min_size <= self.len() {
            return true;
        }

        // SAFETY: We’re assuming self.account has been constructed from
        // Solana’s runtime data.  Note that AccountInfo::realloc isn’t marked
        // `unsafe` even though it makes the same assumption.  Solana is weird.
        let original_data_len = unsafe { self.account.original_data_len() };
        // To minimise number of reallocations, always increase by maximum
        // allowable step, i.e. 10 KiB.  This gives us space for ~142 additional
        // nodes.
        let new_len = (original_data_len + MAX_PERMITTED_DATA_INCREASE)
            .min(MAX_PERMITTED_DATA_LENGTH as usize);
        if min_size > new_len {
            return false;
        }

        if !self.ensure_minimum_balance(new_len) {
            return false;
        }

        let mut data = match self.account.try_borrow_mut_data() {
            Ok(data) => data,
            Err(_) => return false,
        };
        // SAFETY: Just like above, we’re assuming self.account has been
        // constructed from Solana runtime data.  This code has been copied
        // from AccountInfo::realloc.
        unsafe {
            let data_ptr = data.as_mut_ptr();
            // First set new length in the serialised data
            data_ptr.offset(-8).cast::<u64>().write(new_len as u64);
            // Then recreate the local slice with the new length
            *data = core::slice::from_raw_parts_mut(data_ptr, new_len);
        }

        true
    }
}
