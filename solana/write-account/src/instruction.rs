use solana_program::instruction::AccountMeta;
use solana_program::program_error::ProgramError;
use solana_program::pubkey::Pubkey;

type Result<T = (), E = ProgramError> = core::result::Result<T, E>;

/// Generates instruction data for a Write operation.
///
/// `seed` and `bump` specifies seed and bump of the Write PDA.  Note that the
/// actual seed used to create the PDA is `[payer.key, seed]` rather than just
/// `seed`.  Use [`write_find_account`] to figure out the correct Write account
/// with bump and generate the instruction data.
///
/// `offset` specifies offset in the write account to write data at and `data`
/// is the data to write.
pub fn write(
    seed: &[u8],
    bump: u8,
    offset: usize,
    data: &[u8],
) -> Result<Vec<u8>> {
    let seed_len = check_seed(seed)?;
    let offset = u32::try_from(offset)
        .map_err(|_| ProgramError::ArithmeticOverflow)?
        .to_le_bytes();
    Ok([
        /* discriminant, seed_len: */ &[0, seed_len],
        /* seed: */ seed,
        /* bump, offset: */
        &[bump, offset[0], offset[1], offset[2], offset[3]],
        /* data: */ data,
    ]
    .concat())
}

/// Iterator generating Solana instructions calling the write-account program
/// filling given account with given data.
pub struct WriteIter<'a> {
    write_program: &'a Pubkey,
    payer: Pubkey,
    write_account: Pubkey,
    seed: &'a [u8],
    bump: u8,
    data: Vec<u8>,
    position: usize,
    pub chunk_size: core::num::NonZeroU16,
}

impl<'a> WriteIter<'a> {
    /// Constructs a new iterator generating Write instructions.
    ///
    /// `write_program` is the address of the write-account program used to fill
    /// account with the data.  `payer` is the account which signs and pays for
    /// the transaction and rent on the write account.  `seed` is seed used as
    /// part of the PDA of the write account.  `data` is the data to write into
    /// the account.
    ///
    /// Note that if the write account already exists and is larger than data’s
    /// length, the remaining bytes of the account will be untouched.  The
    /// typical approach is to length-prefix the data.
    ///
    /// Returns iterator which generates Write instructions calling
    /// `write_program` and the address and bump of the write account where the
    /// data will be written to.
    ///
    /// # Example
    ///
    /// ```ignore
    /// let (mut chunks, chunk_account, _) = WriteIter::new(
    ///     &write_account_program_id,
    ///     authority.pubkey(),
    ///     b"",
    ///     instruction_data,
    /// ).unwrap();
    /// for instruction in chunks {
    ///     let transaction = Transaction::new_signed_with_payer(
    ///         &[instruction],
    ///         Some(&chunks.payer),
    ///         &[&authority],
    ///         blockhash,
    ///     );
    ///     sol_rpc_client
    ///         .send_and_confirm_transaction_with_spinner(&transaction)
    ///         .unwrap();
    /// }
    /// ```
    pub fn new(
        write_program: &'a Pubkey,
        payer: Pubkey,
        seed: &'a [u8],
        data: Vec<u8>,
    ) -> Result<(Self, Pubkey, u8)> {
        check_seed(seed)?;
        let (write_account, bump) = Pubkey::find_program_address(
            &[payer.as_ref(), seed],
            write_program,
        );
        let iter = Self {
            write_program,
            payer,
            write_account,
            seed,
            bump,
            data,
            position: 0,
            // TODO(mina86): Figure out the maximum size which would still fit
            // in a transaction.
            chunk_size: core::num::NonZeroU16::new(500).unwrap(),
        };
        Ok((iter, write_account, bump))
    }
}

impl core::iter::Iterator for WriteIter<'_> {
    type Item = solana_program::instruction::Instruction;

    fn next(&mut self) -> Option<Self::Item> {
        let len = self.data.len();
        let start = self.position;
        if start >= len {
            return None;
        }
        let end = start.saturating_add(self.chunk_size.get().into()).min(len);
        self.position = end;
        let chunk = &self.data[start..end];

        let data = [
            /* discriminant: */ b"\0",
            /* seed_len: */ &[self.seed.len() as u8][..],
            /* seed: */ self.seed,
            /* bump: */ &[self.bump],
            /* offset: */
            &u32::try_from(start).unwrap().to_le_bytes()[..],
            /* data: */ chunk,
        ]
        .concat();

        Some(solana_program::instruction::Instruction {
            program_id: *self.write_program,
            accounts: vec![
                AccountMeta::new(self.payer, true),
                AccountMeta::new(self.write_account, false),
                AccountMeta::new(solana_program::system_program::ID, false),
            ],
            data,
        })
    }
}

/// Checks that seed is below the maximum length; returns length cast to `u8`.
fn check_seed(seed: &[u8]) -> Result<u8> {
    if seed.len() <= solana_program::pubkey::MAX_SEED_LEN {
        Ok(seed.len() as u8)
    } else {
        Err(ProgramError::MaxSeedLengthExceeded)
    }
}
