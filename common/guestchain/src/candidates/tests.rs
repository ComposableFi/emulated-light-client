use super::*;
use crate::validators::test_utils::MockPubKey;

fn candidate(pubkey: char, stake: u128) -> Candidate<MockPubKey> {
    Candidate {
        pubkey: MockPubKey(pubkey as u32),
        stake: NonZeroU128::new(stake).unwrap(),
    }
}

#[test]
fn test_rotate() {
    #[track_caller]
    fn run(old_pos: usize, new_pos: usize) -> [u8; 8] {
        let mut arr = [0, 1, 2, 3, 4, 5, 6, 7];
        *rotate(&mut arr[..], old_pos, new_pos) = 42;
        arr
    }

    assert_eq!([42, 1, 2, 3, 4, 5, 6, 7], run(0, 0));
    assert_eq!([0, 1, 2, 3, 4, 5, 6, 42], run(7, 7));
    assert_eq!([1, 2, 3, 4, 5, 6, 7, 42], run(0, 7));
    assert_eq!([42, 0, 1, 2, 3, 4, 5, 6], run(7, 0));
    assert_eq!([0, 1, 3, 4, 5, 42, 6, 7], run(2, 5));
    assert_eq!([0, 1, 42, 2, 3, 4, 6, 7], run(5, 2));
}

#[test]
fn test_ord() {
    use core::cmp::Ordering::*;

    fn test(want: core::cmp::Ordering, rhs: (char, u128), lhs: (char, u128)) {
        let rhs = candidate(rhs.0, rhs.1);
        let lhs = candidate(lhs.0, lhs.1);
        if want == Equal {
            assert_eq!(rhs, lhs);
        } else {
            assert_ne!(rhs, lhs);
        }
        assert_eq!(want, rhs.cmp(&lhs));
    }

    test(Less, ('C', 20), ('A', 10));
    test(Less, ('C', 20), ('C', 10));
    test(Less, ('C', 20), ('A', 10));
    test(Greater, ('C', 20), ('A', 20));
    test(Equal, ('C', 20), ('C', 20));
}

struct Cfg {
    min_validators: u16,
    min_validator_stake: u128,
    min_total_stake: u128,
}

impl Default for Cfg {
    fn default() -> Self {
        Self { min_validators: 1, min_validator_stake: 1, min_total_stake: 1 }
    }
}

impl From<Cfg> for crate::Config {
    fn from(cfg: Cfg) -> Self {
        crate::Config {
            max_validators: NonZeroU16::MAX,
            min_validators: NonZeroU16::new(cfg.min_validators).unwrap(),
            min_validator_stake: NonZeroU128::new(cfg.min_validator_stake)
                .unwrap(),
            min_total_stake: NonZeroU128::new(cfg.min_total_stake).unwrap(),
            min_quorum_stake: NonZeroU128::MIN,
            min_block_length: crate::height::HostDelta::from(1),
            max_block_age_ns: u64::MAX,
            min_epoch_length: crate::height::HostDelta::from(1),
        }
    }
}

fn cfg_with_min_validators(min_validators: u16) -> crate::Config {
    Cfg { min_validators, ..Default::default() }.into()
}

fn cfg_with_min_validator_stake(min_validator_stake: u128) -> crate::Config {
    Cfg { min_validator_stake, ..Default::default() }.into()
}

fn cfg_with_min_total_stake(min_total_stake: u128) -> crate::Config {
    Cfg { min_total_stake, ..Default::default() }.into()
}

#[track_caller]
fn check<const N: usize>(
    want_candidates: [(char, u128); N],
    candidates: &Candidates<MockPubKey>,
) {
    let max = usize::from(candidates.max_validators.get());
    let want_stake =
        want_candidates.iter().take(max).map(|(_, stake)| stake).sum::<u128>();
    let want_candidates = want_candidates
        .into_iter()
        .map(|(pubkey, stake)| candidate(pubkey, stake))
        .collect::<Vec<_>>();
    assert_eq!(
        (want_stake, want_candidates.as_slice()),
        (candidates.head_stake, candidates.candidates.as_slice())
    )
}

#[track_caller]
fn stake_setter(
    pubkey: char,
    old_stake: u128,
    new_stake: u128,
) -> impl Fn(Option<&Candidate<MockPubKey>>) -> Result<u128, UpdateCandidateError>
{
    move |got| {
        let want = NonZeroU128::new(old_stake).map(|stake| {
            let pubkey = MockPubKey(pubkey as u32);
            Candidate { pubkey, stake }
        });
        assert_eq!(want.as_ref(), got);
        Ok(new_stake)
    }
}

#[test]
fn test_candidates_0() {
    use candidate as c;
    use UpdateCandidateError::*;

    fn pk(pubkey: char) -> MockPubKey { MockPubKey(pubkey as u32) }

    // Create candidates set
    let mut candidates = Candidates::from_candidates(
        NonZeroU16::new(3).unwrap(),
        [c('A', 1), c('B', 2), c('C', 3), c('D', 4), c('E', 5)].to_vec(),
    );
    check([('E', 5), ('D', 4), ('C', 3), ('B', 2), ('A', 1)], &candidates);

    // Check minimum total stake and count are checked
    assert_eq!(
        Err(NotEnoughTotalStake),
        candidates.update(
            &cfg_with_min_total_stake(10),
            pk('E'),
            stake_setter('E', 5, 0)
        ),
    );
    assert_eq!(
        Err(NotEnoughValidators),
        candidates.update(
            &cfg_with_min_validators(5),
            pk('E'),
            stake_setter('E', 5, 0)
        ),
    );

    // Removal is idempotent
    candidates
        .update(&cfg_with_min_validators(2), pk('E'), stake_setter('E', 5, 0))
        .unwrap();
    check([('D', 4), ('C', 3), ('B', 2), ('A', 1)], &candidates);
    candidates
        .update(&cfg_with_min_validators(2), pk('E'), stake_setter('E', 0, 0))
        .unwrap();
    check([('D', 4), ('C', 3), ('B', 2), ('A', 1)], &candidates);

    // Go below max_validators of candidates.
    candidates
        .update(&cfg_with_min_validators(1), pk('C'), stake_setter('C', 3, 0))
        .unwrap();
    candidates
        .update(&cfg_with_min_validators(1), pk('B'), stake_setter('B', 2, 0))
        .unwrap();
    candidates
        .update(&cfg_with_min_validators(1), pk('A'), stake_setter('A', 1, 0))
        .unwrap();
    check([('D', 4)], &candidates);

    // Minimum validator stake is checked
    assert_eq!(
        Err(NotEnoughValidatorStake),
        candidates.update(
            &cfg_with_min_validator_stake(4),
            pk('C'),
            stake_setter('C', 0, 3)
        ),
    );

    // Add back to have over max.  Minimums are not checked since we’re
    // adding candidates and stake.  This theoretically may be a situation
    // after chain configuration change so we need to support it.
    candidates
        .update(&cfg_with_min_total_stake(20), pk('A'), stake_setter('A', 0, 3))
        .unwrap();
    candidates
        .update(&cfg_with_min_total_stake(20), pk('B'), stake_setter('B', 0, 2))
        .unwrap();
    candidates
        .update(&cfg_with_min_total_stake(20), pk('C'), stake_setter('C', 0, 3))
        .unwrap();
    check([('D', 4), ('A', 3), ('C', 3), ('B', 2)], &candidates);

    // Increase stake.  Again, minimums are not checked.
    candidates
        .update(&cfg_with_min_total_stake(20), pk('C'), stake_setter('C', 3, 4))
        .unwrap();
    check([('C', 4), ('D', 4), ('A', 3), ('B', 2)], &candidates);

    // Reduce stake.  Now, minimums are checked.
    assert_eq!(
        Err(NotEnoughValidatorStake),
        candidates.update(
            &cfg_with_min_validator_stake(3),
            pk('C'),
            stake_setter('C', 4, 2)
        ),
    );
    assert_eq!(
        Err(NotEnoughTotalStake),
        candidates.update(
            &cfg_with_min_total_stake(10),
            pk('C'),
            stake_setter('C', 4, 2)
        ),
    );
    check([('C', 4), ('D', 4), ('A', 3), ('B', 2)], &candidates);

    candidates
        .update(&cfg_with_min_total_stake(10), pk('B'), stake_setter('B', 2, 3))
        .unwrap();
    check([('C', 4), ('D', 4), ('A', 3), ('B', 3)], &candidates);
    // `C` is moved out of validators but incoming `B` candidate has enough
    // stake to meet min total stake limit.
    candidates
        .update(&cfg_with_min_total_stake(10), pk('C'), stake_setter('C', 4, 2))
        .unwrap();
    check([('D', 4), ('A', 3), ('B', 3), ('C', 2)], &candidates);
}

#[test]
fn test_candidiates_1() {
    use candidate as c;

    let mut candidates = Candidates::from_candidates(
        NonZeroU16::new(4).unwrap(),
        [c('F', 168), c('D', 95), c('E', 81), c('C', 68), c('I', 63)].to_vec(),
    );

    let cfg = TestCtx::make_config();
    candidates
        .update(&cfg, MockPubKey('I' as u32), |_| {
            Result::<_, UpdateCandidateError>::Ok(254)
        })
        .unwrap();
    check(
        [('I', 254), ('F', 168), ('D', 95), ('E', 81), ('C', 68)],
        &candidates,
    );
}

struct TestCtx {
    config: crate::Config,
    candidates: Candidates<MockPubKey>,
    by_key: alloc::collections::BTreeMap<MockPubKey, u128>,
}

impl TestCtx {
    /// Generates a new candidates set with random set of candidiates.
    fn new(rng: &mut impl rand::Rng) -> Self {
        let config = Self::make_config();

        let candidates = (0..150)
            .map(|idx| {
                let pubkey = MockPubKey(idx);
                let stake = NonZeroU128::new(rng.gen_range(100..255)).unwrap();
                Candidate { pubkey, stake }
            })
            .collect::<Vec<_>>();

        let by_key = candidates
            .iter()
            .map(|c| (c.pubkey, c.stake.get()))
            .collect::<alloc::collections::BTreeMap<_, _>>();

        let candidates =
            Candidates::from_candidates(config.max_validators, candidates);

        Self { config, candidates, by_key }
    }

    /// Generates a test config.
    fn make_config() -> crate::Config {
        let mut config = crate::Config::from(Cfg {
            min_validators: 64,
            min_validator_stake: 128,
            min_total_stake: 16000,
        });
        config.max_validators = NonZeroU16::new(128).unwrap();
        config
    }

    /// Checks that total stake and number of validators respect the limits from
    /// configuration file.
    fn check(&self) {
        assert!(
            self.candidates.candidates.len() >=
                usize::from(self.config.min_validators.get()),
            "Violated min validators constraint: {} < {}",
            self.candidates.candidates.len(),
            self.config.min_validators.get(),
        );
        assert!(
            self.candidates.head_stake >= self.config.min_total_stake.get(),
            "Violated min total stake constraint: {} < {}",
            self.candidates.head_stake,
            self.config.min_total_stake.get(),
        );
    }

    /// Attempts to removes a candidate from candidates set and verifies result
    /// of the operation.
    fn test_remove(&mut self, pubkey: MockPubKey) {
        use super::UpdateCandidateError::*;

        let count = self.candidates.candidates.len();
        let head_stake = self.candidates.head_stake;

        let res =
            self.candidates.update(&self.config, pubkey.clone(), |_| Ok(0));
        self.check();

        if let Err(err) = res {
            let old_stake = self.by_key.get(&pubkey).unwrap().clone();
            assert_eq!(count, self.candidates.candidates.len());
            assert_eq!(head_stake, self.candidates.head_stake);

            match err {
                NotEnoughValidatorStake => unreachable!(),
                NotEnoughTotalStake => {
                    // What would be promoted candidate’s stake after
                    // removal.
                    let new_stake = self
                        .candidates
                        .candidates
                        .get(usize::from(self.config.max_validators.get()))
                        .map_or(0, |c: &Candidate<_>| c.stake.get());
                    assert!(
                        head_stake - old_stake + new_stake <
                            self.config.min_total_stake.get()
                    );
                }
                NotEnoughValidators => {
                    assert!(
                        self.candidates.candidates.len() <=
                            usize::from(self.config.min_validators.get())
                    );
                }
            }
        } else if self.by_key.remove(&pubkey).is_some() {
            assert_eq!(count - 1, self.candidates.candidates.len());
            assert!(head_stake >= self.candidates.head_stake);
        } else {
            assert_eq!(count, self.candidates.candidates.len());
            assert_eq!(head_stake, self.candidates.head_stake);
        }
    }

    /// Attempts to update candidate’s stake and verifies result of the
    /// operation.
    fn test_update(&mut self, pubkey: MockPubKey, new_stake: u128) {
        use alloc::collections::btree_map::Entry;

        let count = self.candidates.candidates.len();
        let head_stake = self.candidates.head_stake;

        let res = self
            .candidates
            .update(&self.config, pubkey.clone(), |_| Ok(new_stake));
        self.check();

        if let Err(err) = res {
            assert_eq!(count, self.candidates.candidates.len());
            assert_eq!(head_stake, self.candidates.head_stake);
            self.verify_update_error(err, pubkey, new_stake);
        } else {
            let entry = self.by_key.entry(pubkey.clone());
            let new = matches!(&entry, Entry::Vacant(_));
            assert_eq!(
                count + usize::from(new),
                self.candidates.candidates.len()
            );
            if new {
                assert!(head_stake <= self.candidates.head_stake);
            }
            *entry.or_default() = new_stake;
        }
    }

    /// Verifies failed attempt at updating candidate’s stake.
    fn verify_update_error(
        &self,
        err: UpdateCandidateError,
        pubkey: MockPubKey,
        new_stake: u128,
    ) {
        use super::UpdateCandidateError::*;

        match err {
            NotEnoughValidatorStake => {
                assert!(new_stake < self.config.min_validator_stake.get());
                return;
            }
            NotEnoughValidators => unreachable!(),
            NotEnoughTotalStake => (),
        }

        let old_stake = self.by_key.get(&pubkey).unwrap();

        // There are two possibilities.  We are in head and would stay there or
        // we would be moved outside of it (replaced by whoever is just past the
        // head).  We can determine those cases by comparing updated state to
        // the state just outside the head.
        let last = self
            .candidates
            .candidates
            .get(usize::from(self.config.max_validators.get()));
        let kicked_out = last.clone().map_or(false, |candidiate| {
            candidiate <
                &Candidate {
                    pubkey,
                    stake: NonZeroU128::new(new_stake).unwrap(),
                }
        });

        let new_stake =
            if kicked_out { last.unwrap().stake.get() } else { new_stake };

        assert!(
            self.candidates.head_stake - old_stake + new_stake <
                self.config.min_total_stake.get()
        );
    }

    /// Performs a random test.  `data` must be a two-element slice.  The random
    /// test is determined from values in the slice.
    fn test(&mut self, pubkey: u8, stake: u8) {
        let old_state = self.candidates.clone();
        let pubkey = MockPubKey(u32::from(pubkey));

        let this = self as *mut TestCtx;
        let res = std::panic::catch_unwind(|| {
            // SAFETY: It’s test code.  I don’t care. ;)  It’s probably safe but
            // self.candidates may be in inconsistent state.  This is fine since
            // we’re panicking anyway.
            let this = unsafe { &mut *this };
            match stake {
                0 => this.test_remove(pubkey),
                _ => this.test_update(pubkey.clone(), u128::from(stake)),
            }
        });

        if let Err(err) = res {
            std::eprintln!("{:?}", old_state);
            match stake {
                0 => std::eprintln!(" Remove {pubkey:?}"),
                _ => std::eprintln!(" Update {pubkey:?} staking {stake}"),
            }
            std::eprintln!("{:?}", self.candidates);
            std::panic::resume_unwind(err);
        }
    }
}

#[test]
fn stress_test() {
    use rand::Rng;

    let mut rng = rand::thread_rng();
    let mut ctx = TestCtx::new(&mut rng);
    let mut n = lib::test_utils::get_iteration_count(1);
    let mut buf = [0u8; 2 * 1024];
    while n > 0 {
        rng.fill(&mut buf[..]);
        for data in buf.chunks_exact(2).take(n) {
            ctx.test(data[0], data[1]);
        }
        n = n.saturating_sub(1024);
    }
}
