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
pub const SOL_PRICE_FEED_ID: &str =
    "0xef0d8b6fda2ceba41da15d4095d1da392a0d2f8ed0c6c7bc0f4cfac8c280b56d";
pub const SOL_DECIMALS: u8 = 9;

/// Period of time funds are held until they can be withdrawn.
///
/// Currently set to seven days.  However, when code is compiled with `mocks`
/// feature enabled it’s set to one second for testing.
pub const UNBONDING_PERIOD_IN_SEC: u64 =
    if cfg!(feature = "mocks") { 1 } else { 7 * 24 * 60 * 60 };
