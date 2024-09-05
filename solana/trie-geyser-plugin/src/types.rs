use core::str::FromStr;

use geyser_plugin_interface::ReplicaAccountInfoVersions;
use solana_geyser_plugin_interface::geyser_plugin_interface::{
    self, GeyserPluginError, ReplicaBlockInfoVersions,
};
use solana_sdk::hash::Hash;

use crate::utils;


// =============================================================================
// Account information

/// Account information extracted from the data sent to the plugin.
///
/// The type provides conversion from [`ReplicaAccountInfoVersions`] allowing it
/// to be easily used in the plugin code.
#[derive(Debug, Clone)]
pub struct AccountInfo<'a> {
    pub lamports: u64,
    pub owner: &'a [u8; 32],
    pub executable: bool,
    pub rent_epoch: u64,
    pub data: &'a [u8],
    pub write_version: u64,
    pub pubkey: &'a [u8; 32],
}

impl<'a> TryFrom<&'a geyser_plugin_interface::ReplicaAccountInfo<'a>>
    for AccountInfo<'a>
{
    type Error = core::array::TryFromSliceError;

    fn try_from(
        acc: &'a geyser_plugin_interface::ReplicaAccountInfo<'a>,
    ) -> Result<Self, Self::Error> {
        Ok(Self {
            lamports: acc.lamports,
            owner: acc.owner.try_into()?,
            executable: acc.executable,
            rent_epoch: acc.rent_epoch,
            data: acc.data,
            write_version: acc.write_version,
            pubkey: acc.pubkey.try_into()?,
        })
    }
}

impl<'a> TryFrom<&'a geyser_plugin_interface::ReplicaAccountInfoV2<'a>>
    for AccountInfo<'a>
{
    type Error = core::array::TryFromSliceError;

    fn try_from(
        acc: &'a geyser_plugin_interface::ReplicaAccountInfoV2<'a>,
    ) -> Result<Self, Self::Error> {
        Ok(Self {
            lamports: acc.lamports,
            owner: acc.owner.try_into()?,
            executable: acc.executable,
            rent_epoch: acc.rent_epoch,
            data: acc.data,
            write_version: acc.write_version,
            pubkey: acc.pubkey.try_into()?,
        })
    }
}

impl<'a> TryFrom<&'a geyser_plugin_interface::ReplicaAccountInfoV3<'a>>
    for AccountInfo<'a>
{
    type Error = core::array::TryFromSliceError;

    fn try_from(
        acc: &'a geyser_plugin_interface::ReplicaAccountInfoV3<'a>,
    ) -> Result<Self, Self::Error> {
        Ok(Self {
            lamports: acc.lamports,
            owner: acc.owner.try_into()?,
            executable: acc.executable,
            rent_epoch: acc.rent_epoch,
            data: acc.data,
            write_version: acc.write_version,
            pubkey: acc.pubkey.try_into()?,
        })
    }
}

impl<'a> TryFrom<ReplicaAccountInfoVersions<'a>> for AccountInfo<'a> {
    type Error = GeyserPluginError;

    fn try_from(
        acc: ReplicaAccountInfoVersions<'a>,
    ) -> Result<Self, Self::Error> {
        match acc {
            ReplicaAccountInfoVersions::V0_0_1(acc) => acc.try_into(),
            ReplicaAccountInfoVersions::V0_0_2(acc) => acc.try_into(),
            ReplicaAccountInfoVersions::V0_0_3(acc) => acc.try_into(),
        }
        .map_err(|_| GeyserPluginError::AccountsUpdateError {
            msg: "invalid pubkey length".into(),
        })
    }
}

impl<'a> From<AccountInfo<'a>> for cf_solana::proof::AccountHashData {
    fn from(info: AccountInfo<'a>) -> Self {
        Self::new(
            info.lamports,
            info.owner.into(),
            info.executable,
            info.rent_epoch,
            info.data,
            info.pubkey.into(),
        )
    }
}

// =============================================================================
// Block information

/// Block information extracted from the data sent to the plugin.
///
/// The type provides conversion from [`ReplicaBlockInfoVersions`] allowing it
/// to be easily used in the plugin code.
#[derive(Debug, Clone)]
pub struct BlockInfo {
    pub parent_blockhash: Hash,
    pub blockhash: Hash,
    pub executed_transaction_count: u64,
}

/// Block information extracted from the data sent to the plugin including slot
/// number.
///
/// This is separated from [`BlockInfo`] so that slot → block info mappings
/// don’t store slot twice.
#[derive(Debug, Clone)]
pub struct BlockInfoWithSlot {
    pub slot: u64,
    pub info: BlockInfo,
}

impl<'a> TryFrom<&'a geyser_plugin_interface::ReplicaBlockInfoV2<'a>>
    for BlockInfoWithSlot
{
    type Error = solana_sdk::hash::ParseHashError;

    fn try_from(
        block: &'a geyser_plugin_interface::ReplicaBlockInfoV2<'a>,
    ) -> Result<Self, Self::Error> {
        Ok(Self {
            slot: block.slot,
            info: BlockInfo {
                parent_blockhash: Hash::from_str(block.parent_blockhash)?,
                blockhash: Hash::from_str(block.blockhash)?,
                executed_transaction_count: block.executed_transaction_count,
            },
        })
    }
}

impl<'a> TryFrom<&'a geyser_plugin_interface::ReplicaBlockInfoV3<'a>>
    for BlockInfoWithSlot
{
    type Error = solana_sdk::hash::ParseHashError;

    fn try_from(
        block: &'a geyser_plugin_interface::ReplicaBlockInfoV3<'a>,
    ) -> Result<Self, Self::Error> {
        Ok(Self {
            slot: block.slot,
            info: BlockInfo {
                parent_blockhash: Hash::from_str(block.parent_blockhash)?,
                blockhash: Hash::from_str(block.blockhash)?,
                executed_transaction_count: block.executed_transaction_count,
            },
        })
    }
}

impl<'a> TryFrom<ReplicaBlockInfoVersions<'a>> for BlockInfoWithSlot {
    type Error = GeyserPluginError;

    fn try_from(
        block: ReplicaBlockInfoVersions<'a>,
    ) -> Result<Self, Self::Error> {
        match block {
            ReplicaBlockInfoVersions::V0_0_1(_) => {
                return Err(utils::custom_err(
                    "ReplicaBlockInfoV1 unsupported",
                ));
            }
            ReplicaBlockInfoVersions::V0_0_2(block) => block.try_into(),
            ReplicaBlockInfoVersions::V0_0_3(block) => block.try_into(),
        }
        .map_err(utils::custom_err)
    }
}
