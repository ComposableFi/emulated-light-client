use solana_geyser_plugin_interface::geyser_plugin_interface;
use solana_sdk::pubkey::Pubkey;

#[derive(Debug, Clone)]
pub struct Config {
    pub root_account: Pubkey,
    pub witness_account: Pubkey,
}

#[derive(Debug, Clone, serde::Deserialize)]
#[serde(deny_unknown_fields)]
struct RawConfig {
    #[serde(rename = "libpath")]
    _libpath: String,
    #[serde(deserialize_with = "deserialize_pubkey")]
    trie_program: Pubkey,
    #[serde(deserialize_with = "deserialize_pubkey")]
    root_account: Pubkey,
}

#[derive(Debug, derive_more::From, derive_more::Display)]
pub enum Error {
    IO(std::io::Error),
    Parse(serde_json::Error),
    #[display(
        fmt = "Unable to find witness account for trie {} owned by {}",
        root_account,
        program_id
    )]
    UnableToFindWitnessAccount {
        program_id: Pubkey,
        root_account: Pubkey,
    },
}

impl Config {
    /// Loads configuration from a file.
    pub fn load(file: &std::path::Path) -> Result<Self, Error> {
        let rd = std::fs::File::open(file)?;
        let cfg: RawConfig = serde_json::from_reader(rd)?;
        let (witness_account, _) = wittrie::api::find_witness_account(
            &cfg.trie_program,
            &cfg.root_account,
        )
        .ok_or_else(|| Error::UnableToFindWitnessAccount {
            program_id: cfg.trie_program,
            root_account: cfg.root_account,
        })?;
        Ok(Self { root_account: cfg.root_account, witness_account })
    }
}

impl From<Error> for geyser_plugin_interface::GeyserPluginError {
    fn from(err: Error) -> Self {
        match err {
            Error::IO(err) => err.into(),
            err => Self::ConfigFileReadError { msg: err.to_string() },
        }
    }
}

fn deserialize_pubkey<'de, D: serde::Deserializer<'de>>(
    de: D,
) -> Result<Pubkey, D::Error> {
    let value: String = serde::Deserialize::deserialize(de)?;
    core::str::FromStr::from_str(&value).map_err(serde::de::Error::custom)
}
