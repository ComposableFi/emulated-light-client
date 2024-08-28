use cf_solana::types::PubKey;
use solana_geyser_plugin_interface::geyser_plugin_interface;
use solana_sdk::pubkey::Pubkey;

#[derive(Debug, Clone, serde::Deserialize)]
#[serde(deny_unknown_fields)]
struct FileConfig {
    #[serde(rename = "libpath")]
    _libpath: String,
    trie_program: PubKey,
    root_account: PubKey,
    bind_address: std::net::SocketAddr,
}

#[derive(Debug, Clone)]
pub struct Config {
    pub root_account: Pubkey,
    pub witness_account: Pubkey,
    pub bind_address: std::net::SocketAddr,
}

#[derive(Debug, derive_more::From, derive_more::Display)]
pub enum Error {
    IO(std::io::Error),
    Parse(serde_json::Error),
    #[display(
        fmt = "Unable to find witness account for trie {} owned by {}",
        root_account,
        trie_program
    )]
    UnableToFindWitnessAccount {
        trie_program: Pubkey,
        root_account: Pubkey,
    },
}

impl Config {
    /// Loads configuration from a file.
    pub fn load(file: &std::path::Path) -> Result<Self, Error> {
        let cfg: FileConfig =
            serde_json::from_reader(std::fs::File::open(file)?)?;
        let trie_program = Pubkey::from(cfg.trie_program);
        let root_account = Pubkey::from(cfg.root_account);
        let bind_address = cfg.bind_address;
        let (witness_account, _) =
            wittrie::api::find_witness_account(&trie_program, &root_account)
                .ok_or_else(|| Error::UnableToFindWitnessAccount {
                    trie_program,
                    root_account,
                })?;
        Ok(Self { root_account, witness_account, bind_address })
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
