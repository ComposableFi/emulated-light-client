use alloc::vec::Vec;
use core::num::{NonZeroU128, NonZeroU16};

#[cfg(test)]
mod tests;

/// Set of candidate validators to consider when creating a new epoch.
///
/// Whenever epoch changes, candidates with most stake are included in
/// validators set.
#[derive(
    Clone, PartialEq, Eq, borsh::BorshSerialize, borsh::BorshDeserialize,
)]
pub struct Candidates<PK> {
    /// Maximum number of validators in a validator set.
    max_validators: NonZeroU16,

    /// Set of validators which are interested in participating in the
    /// blockchain.
    ///
    /// The vector is kept sorted with candidates with most stake first.
    pub candidates: Vec<Candidate<PK>>,

    /// Whether the set changed in a way which affects the epoch.
    ///
    /// If this is true, as soon as possible a new epoch will be started.
    changed: bool,

    /// Sum of the top `max_validators` stakes.
    head_stake: u128,
}

/// A candidate to become a validator.
#[derive(
    Clone, PartialEq, Eq, borsh::BorshSerialize, borsh::BorshDeserialize,
)]
pub struct Candidate<PK> {
    /// Public key of the candidate.
    pub pubkey: PK,

    /// Candidate’s stake.
    pub stake: NonZeroU128,
}

/// Error while updating candidate.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum UpdateCandidateError {
    /// Candidate’s stake is below required minimum.
    NotEnoughValidatorStake,

    /// After removing a candidate or reducing candidate’s stake, the total
    /// stake would fall below required minimum.
    NotEnoughTotalStake,

    /// After removing a candidate, the total number of validators would fall
    /// below required minimum.
    NotEnoughValidators,
}

impl<PK: crate::PubKey> Candidates<PK> {
    /// Creates a new candidates set from the given list.
    ///
    /// If the list is longer than `max_validators` marks the set as `changed`.
    /// I.e. on the next epoch change, the validators set will be changed to
    /// only the top `max_validators`.
    ///
    /// Note that the value of `max_validators` is preserved.  All methods which
    /// take `cfg: &crate::Config` as an argument ignore `cfg.max_validators`
    /// value and use value of this `max_validators` argument instead.
    pub fn new(
        max_validators: NonZeroU16,
        validators: &[crate::Validator<PK>],
    ) -> Self {
        Self::from_candidates(
            max_validators,
            validators.iter().map(Candidate::from).collect::<Vec<_>>(),
        )
    }

    fn from_candidates(
        max_validators: NonZeroU16,
        mut candidates: Vec<Candidate<PK>>,
    ) -> Self {
        candidates.sort_unstable();
        // If validator set in the genesis block is larger than maximum size
        // specified in configuration, than we need to reduce the number on next
        // epoch change.
        let changed = candidates.len() > usize::from(max_validators.get());
        let head_stake = Self::sum_head_stake(max_validators, &candidates);
        let this = Self { max_validators, candidates, changed, head_stake };
        this.debug_verify_state();
        this
    }

    /// Sums stake of the first `count` candidates.
    fn sum_head_stake(count: NonZeroU16, candidates: &[Candidate<PK>]) -> u128 {
        let count = usize::from(count.get()).min(candidates.len());
        candidates[..count]
            .iter()
            .fold(0, |sum, c| sum.checked_add(c.stake.get()).unwrap())
    }

    /// Returns top validators if changed since last time changed flag was
    /// cleared.
    ///
    /// To clear changed flag, use [`Self::clear_changed_flag`].
    pub fn maybe_get_head(&self) -> Option<Vec<crate::Validator<PK>>> {
        self.changed.then(|| {
            self.candidates
                .iter()
                .take(self.max_validators())
                .map(crate::Validator::from)
                .collect::<Vec<_>>()
        })
    }

    /// Clears the changed flag.
    ///
    /// Changed flag is set automatically whenever head of the candidates list
    /// is modified (note that changes outside of the head of candidates list do
    /// not affect the flag).
    pub fn clear_changed_flag(&mut self) { self.changed = false; }

    /// Adds a new candidates or updates existing candidate’s stake.
    ///
    /// The `new_stake_fn` callback takes existing candidate or `None` (if
    /// candidate with given `pubkey` doesn’t exist) as the argument and returns
    /// the new stake for that candidate (or for a new candidate).  If the new
    /// stake is zero, the candidate is removed.
    pub fn update<F, E>(
        &mut self,
        cfg: &crate::Config,
        pubkey: PK,
        new_stake_fn: F,
    ) -> Result<(), E>
    where
        F: FnOnce(Option<&Candidate<PK>>) -> Result<u128, E>,
        E: From<UpdateCandidateError>,
    {
        let pos = self.candidates.iter().position(|el| el.pubkey == pubkey);
        let stake = new_stake_fn(pos.map(|pos| &self.candidates[pos]))?;
        let res = if let Some(stake) = NonZeroU128::new(stake) {
            if stake < cfg.min_validator_stake {
                Err(UpdateCandidateError::NotEnoughValidatorStake)
            } else {
                self.do_update(cfg, pos, Candidate { pubkey, stake })
            }
        } else if let Some(pos) = pos {
            self.do_remove(cfg, pos)
        } else {
            Ok(())
        };
        self.debug_verify_state();
        res.map_err(E::from)
    }

    /// Adds a new candidates or updates existing candidate’s stake.
    fn do_update(
        &mut self,
        cfg: &crate::Config,
        old_pos: Option<usize>,
        candidate: Candidate<PK>,
    ) -> Result<(), UpdateCandidateError> {
        let mut new_pos =
            self.candidates.binary_search(&candidate).unwrap_or_else(|p| p);
        if let Some(old_pos) = old_pos {
            if new_pos > old_pos {
                new_pos -= 1;
            }
            self.update_impl(cfg, old_pos, new_pos, candidate)
        } else {
            self.add_impl(new_pos, candidate);
            Ok(())
        }
    }

    /// Removes an existing candidate.
    fn do_remove(
        &mut self,
        cfg: &crate::Config,
        pos: usize,
    ) -> Result<(), UpdateCandidateError> {
        if self.candidates.len() <= usize::from(cfg.min_validators.get()) {
            return Err(UpdateCandidateError::NotEnoughValidators);
        }
        self.update_stake_for_remove(cfg, pos)?;
        self.candidates.remove(pos);
        Ok(())
    }

    /// Adds a new candidate at given position.
    ///
    /// It’s caller’s responsibility to guarantee that `new_pos` is correct
    /// position for the `candidate` to be added and that there’s no candidate
    /// with the same public key already on the list.
    fn add_impl(&mut self, new_pos: usize, candidate: Candidate<PK>) {
        let new = candidate.stake.get();
        let max = self.max_validators();
        self.candidates.insert(new_pos, candidate);
        if new_pos < max {
            let old = self.candidates.get(max).map_or(0, |c| c.stake.get());
            self.add_head_stake(new - old);
        }
    }

    /// Updates a candidate by changing its position and stake.
    fn update_impl(
        &mut self,
        cfg: &crate::Config,
        old_pos: usize,
        new_pos: usize,
        candidate: Candidate<PK>,
    ) -> Result<(), UpdateCandidateError> {
        let max = self.max_validators();
        if new_pos >= max {
            // Candidate’s new position is outside of the first max_validators.
            // Verify it the same way we verify removal of a candidate since
            // in next epoch they won’t be in the validators set.
            self.update_stake_for_remove(cfg, old_pos)?;
        } else if old_pos >= max {
            // The candidate graduates to the top max_validators.  This may
            // change head_stake but never by decreasing it.
            let new = candidate.stake.get();
            let old = self.candidates.get(max - 1).map_or(0, |c| c.stake.get());
            self.add_head_stake(new - old);
        } else {
            // The candidate moves within the top max_validators.  We need to
            // update head_stake.
            let old_stake = self.candidates[old_pos].stake.get();
            let new_stake = candidate.stake.get();
            if old_stake < new_stake {
                self.add_head_stake(new_stake - old_stake);
            } else if old_stake > new_stake {
                self.sub_head_stake(cfg, old_stake - new_stake)?;
            } else {
                return Ok(());
            };
        }
        rotate(self.candidates.as_mut_slice(), old_pos, new_pos).stake =
            candidate.stake;
        Ok(())
    }

    /// Verifies whether removing candidate at given position adheres to
    /// configuration and, if it does, updates head stake if necessary.
    ///
    /// Returns an error, if removing validator at given position would reduce
    /// number of candidates or stake of the head candidates below minimums from
    /// the configuration.
    ///
    /// Otherwise, acts as if the candidate at the position got removed and
    /// updates `self.head_stake` and `self.changed` if necessary.
    fn update_stake_for_remove(
        &mut self,
        cfg: &crate::Config,
        pos: usize,
    ) -> Result<(), UpdateCandidateError> {
        let max = self.max_validators();
        if pos >= max {
            return Ok(());
        }
        let old = self.candidates[pos].stake.get();
        let new = self.candidates.get(max).map_or(0, |c| c.stake.get());
        self.sub_head_stake(cfg, old - new)
    }

    /// Adds given amount of stake to `head_stake`.
    fn add_head_stake(&mut self, stake: u128) {
        self.head_stake = self.head_stake.checked_add(stake).unwrap();
        self.changed = true;
    }

    /// Subtracts given amount of stake from `head_stake`.
    fn sub_head_stake(
        &mut self,
        cfg: &crate::Config,
        stake: u128,
    ) -> Result<(), UpdateCandidateError> {
        let head_stake = self.head_stake.checked_sub(stake).unwrap();
        if head_stake < cfg.min_total_stake.get() {
            return Err(UpdateCandidateError::NotEnoughTotalStake);
        }
        self.head_stake = head_stake;
        self.changed = true;
        Ok(())
    }

    /// If debug assertions are enabled, checks whether all invariants are held.
    ///
    /// Verifies that a) candidates are sorted, b) contain no duplicates and c)
    /// `self.head_stake` is sum of stake of first `self.max_validators`
    /// candidates.
    #[track_caller]
    fn debug_verify_state(&self) {
        if !cfg!(debug_assertions) {
            return;
        }
        for (idx, wnd) in self.candidates.windows(2).enumerate() {
            assert!(wnd[0] < wnd[1], "{idx}");
        }

        let mut pks = self
            .candidates
            .iter()
            .map(|c| c.pubkey.clone())
            .collect::<Vec<_>>();
        pks.sort_unstable();
        for wnd in pks.windows(2) {
            assert!(wnd[0] != wnd[1]);
        }

        let got = Self::sum_head_stake(self.max_validators, &self.candidates);
        assert_eq!(self.head_stake, got);
    }
}

impl<PK> Candidates<PK> {
    /// Convenience method which returns `self.max_validators` as `usize`.
    fn max_validators(&self) -> usize { usize::from(self.max_validators.get()) }
}

/// Rotates subslice such that element at `old_pos` moves to `new_pos`.
///
/// Depending whether `old_pos` is less than or greater than `new_pos`, performs
/// a left or right rotation of a `min(old_pos, new_pos)..=max(old_pos,
/// new_pos)` subslice.
///
/// Returns reference to the element at `new_pos`.
fn rotate<T>(slice: &mut [T], old_pos: usize, new_pos: usize) -> &mut T {
    use core::cmp::Ordering;
    match old_pos.cmp(&new_pos) {
        Ordering::Less => slice[old_pos..=new_pos].rotate_left(1),
        Ordering::Equal => (),
        Ordering::Greater => slice[new_pos..=old_pos].rotate_right(1),
    }
    &mut slice[new_pos]
}

impl<PK: PartialOrd> core::cmp::PartialOrd<Candidate<PK>> for Candidate<PK> {
    /// Compares two candidates sorting by `(-stake, pubkey)` pair.
    ///
    /// That is orders candidates by their stake in descending order and (in
    /// case of equal stakes) by public key in ascending order.
    fn partial_cmp(&self, rhs: &Self) -> Option<core::cmp::Ordering> {
        match rhs.stake.cmp(&self.stake) {
            core::cmp::Ordering::Equal => self.pubkey.partial_cmp(&rhs.pubkey),
            ord => Some(ord),
        }
    }
}

impl<PK: Ord> core::cmp::Ord for Candidate<PK> {
    /// Compares two candidates sorting by `(-stake, pubkey)` pair.
    ///
    /// That is orders candidates by their stake in descending order and (in
    /// case of equal stakes) by public key in ascending order.
    fn cmp(&self, rhs: &Self) -> core::cmp::Ordering {
        rhs.stake.cmp(&self.stake).then_with(|| self.pubkey.cmp(&rhs.pubkey))
    }
}

impl<PK: crate::PubKey> From<&Candidate<PK>> for crate::Validator<PK> {
    fn from(candidate: &Candidate<PK>) -> Self {
        Self::new(candidate.pubkey.clone(), candidate.stake)
    }
}

impl<PK: crate::PubKey> From<&crate::Validator<PK>> for Candidate<PK> {
    fn from(validator: &crate::Validator<PK>) -> Self {
        Self { pubkey: validator.pubkey().clone(), stake: validator.stake() }
    }
}

impl<PK: core::fmt::Debug> core::fmt::Debug for Candidate<PK> {
    fn fmt(&self, fmt: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        write!(fmt, "{{{:?} staking {}}}", self.pubkey, self.stake.get())
    }
}

impl<PK: core::fmt::Debug> core::fmt::Debug for Candidates<PK> {
    fn fmt(&self, fmtr: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        fmtr.write_str("{[")?;
        let max = self.max_validators();
        for (index, candidate) in self.candidates.iter().enumerate() {
            let sep = if index == 0 {
                ""
            } else if index == max {
                " | "
            } else {
                ", "
            };
            write!(fmtr, "{sep}{candidate:?}")?;
        }
        let changed = ["", " (changed)"][usize::from(self.changed)];
        write!(fmtr, "]; head_stake: {}{changed}}}", self.head_stake)
    }
}
