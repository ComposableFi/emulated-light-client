pub const STAKING_PARAMS_SEED: &[u8] = b"staking_params";
pub const VAULT_PARAMS_SEED: &[u8] = b"vault_params";
pub const VAULT_SEED: &[u8] = b"vault";
pub const TEST_SEED: &[u8] = b"abcdefg2";
pub const ESCROW_RECEIPT_SEED: &[u8] = b"escrow_receipt";
pub const REWARDS_SEED: &[u8] = b"rewards";

pub const TOKEN_NAME: &str = "Composable Restaking Position";
pub const TOKEN_SYMBOL: &str = "CRP";
pub const TOKEN_URI: &str =
    "https://arweave.net/QbxPlvN1nHFG0AVXfGNdlXUk-LEkrQxFffI3fOUDciA";

#[cfg(not(feature = "mocks"))]
/// 7 days in seconds (60 * 60 * 24 * 7)
pub const UNBONDING_PERIOD_IN_SEC: u64 = 604800;

#[cfg(feature = "mocks")]
/// 1 second for tests
pub const UNBONDING_PERIOD_IN_SEC: u64 = 1;
