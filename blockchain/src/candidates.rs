use alloc::vec::Vec;
use core::num::{NonZeroU128, NonZeroU16};

use crate::chain;
use crate::validators::{PubKey, Validator};

/// Set of candidate validators to consider when creating a new epoch.
pub struct Candidates<PK> {
    /// Maximum number of validators in a validator set.
    max_validators: NonZeroU16,

    /// Set of validators which are interested in participating in the
    /// blockchain.
    candidates: Vec<Candidate<PK>>,

    /// Whether the set changed in a way which affects the epoch.
    ///
    /// If this is true, as soon as possible a new epoch will be started.
    changed: bool,

    /// Sum of the top `max_validators` stakes.
    head_stake: u128,
}

#[derive(Clone, PartialEq, Eq)]
struct Candidate<PK> {
    /// Public key of the candidate.
    pubkey: PK,

    /// Candidate’s stake.
    stake: NonZeroU128,
}

/// Error while updating candidate.
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

impl<PK: PubKey> Candidates<PK> {
    /// Creates a new candidates set from the given list.
    ///
    /// If the list is longer than `max_validators` marks the set as `changed`.
    /// I.e. on the next epoch change, the validators set will be changed to
    /// only the top `max_validators`.
    ///
    /// Note that the value of `max_validators` is preserved.  All methods which
    /// take `cfg: &chain::Config` as an argument ignore `cfg.max_validators`
    /// value and use value of this `max_validators` argument instead.
    pub fn new(
        max_validators: NonZeroU16,
        validators: &[Validator<PK>],
    ) -> Self {
        let mut candidates =
            validators.iter().map(Candidate::from).collect::<Vec<_>>();
        candidates.sort_unstable();
        // If validator set in the genesis block is larger than maximum size
        // specified in configuration, than we need to reduce the number on next
        // epoch change.
        let max = usize::from(max_validators.get());
        let changed = candidates.len() > max;
        let head_stake = candidates
            .iter()
            .take(max)
            .map(|c| c.stake)
            .reduce(|a, b| a.checked_add(b.get()).unwrap())
            .map_or(0, |stake| stake.get());
        Self { max_validators, candidates, changed, head_stake }
    }

    /// Returns top validators together with their total stake if changed since
    /// last call.
    pub fn maybe_get_head(&mut self) -> Option<(Vec<Validator<PK>>, u128)> {
        if !self.changed {
            return None;
        }
        let mut total: u128 = 0;
        let validators = self
            .candidates
            .iter()
            .take(self.max_validators())
            .map(|candidate| {
                total = total.checked_add(candidate.stake.get())?;
                Some(Validator::from(candidate))
            })
            .collect::<Option<Vec<_>>>()
            .unwrap();
        self.changed = false;
        Some((validators, total))
    }

    /// Adds a new candidates or updates existing candidate’s stake.
    pub fn update(
        &mut self,
        cfg: &chain::Config,
        pubkey: PK,
        stake: u128,
    ) -> Result<(), UpdateCandidateError> {
        let stake = NonZeroU128::new(stake)
            .filter(|stake| *stake >= cfg.min_validator_stake)
            .ok_or(UpdateCandidateError::NotEnoughValidatorStake)?;
        let candidate = Candidate { pubkey, stake };
        let old_pos =
            self.candidates.iter().position(|el| el.pubkey == candidate.pubkey);
        let new_pos =
            self.candidates.binary_search(&candidate).map_or_else(|p| p, |p| p);
        match old_pos {
            None => Ok(self.add_impl(new_pos, candidate)),
            Some(old_pos) => self.update_impl(cfg, old_pos, new_pos, candidate),
        }
    }

    /// Removes an existing candidate.
    pub fn remove(
        &mut self,
        cfg: &chain::Config,
        pubkey: &PK,
    ) -> Result<(), UpdateCandidateError> {
        let pos = self.candidates.iter().position(|el| &el.pubkey == pubkey);
        if let Some(pos) = pos {
            if self.candidates.len() <= cfg.min_validators.get().into() {
                return Err(UpdateCandidateError::NotEnoughValidators);
            }
            self.update_stake_for_remove(cfg, pos)?;
            self.candidates.remove(pos);
        }
        Ok(())
    }

    fn max_validators(&self) -> usize { usize::from(self.max_validators.get()) }

    /// Adds a new candidate at given position.
    ///
    /// It’s caller’s responsibility to guarantee that `new_pos` is correct
    /// position for the `candidate` to be added and that there’s no candidate
    /// with the same public key already on the list.
    fn add_impl(&mut self, new_pos: usize, candidate: Candidate<PK>) {
        debug_assert_eq!(
            None,
            self.candidates.iter().position(|c| c.pubkey == candidate.pubkey)
        );
        debug_assert!(new_pos == 0 || self.candidates[new_pos - 1] < candidate);
        debug_assert!(self
            .candidates
            .get(new_pos)
            .map_or(true, |c| &candidate < c));

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
        cfg: &chain::Config,
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
            let old = self.candidates.get(max).map_or(0, |c| c.stake.get());
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
        cfg: &chain::Config,
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
        cfg: &chain::Config,
        stake: u128,
    ) -> Result<(), UpdateCandidateError> {
        let head_stake = self.head_stake.checked_add(stake).unwrap();
        if head_stake < cfg.min_total_stake.get() {
            return Err(UpdateCandidateError::NotEnoughTotalStake);
        }
        self.head_stake = head_stake;
        self.changed = true;
        Ok(())
    }
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

impl<PK: PubKey> From<&Candidate<PK>> for Validator<PK> {
    fn from(candidate: &Candidate<PK>) -> Self {
        Self::new(candidate.pubkey.clone(), candidate.stake)
    }
}

impl<PK: PubKey> From<&Validator<PK>> for Candidate<PK> {
    fn from(validator: &Validator<PK>) -> Self {
        Self { pubkey: validator.pubkey().clone(), stake: validator.stake() }
    }
}
