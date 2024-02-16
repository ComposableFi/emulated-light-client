use std::fmt::{Debug, Display};
use std::fs;
use std::str::FromStr;

use anchor_client::solana_sdk::signature::{
    read_keypair_file, Keypair, Signer,
};
use anchor_lang::solana_program::pubkey::Pubkey;
use clap::{arg, command, Args, Parser, Subcommand};
use dialoguer::theme::ColorfulTheme;
use dialoguer::Input;
use log::LevelFilter;
use serde::de::Error as SerdeError;
use serde::{Deserialize, Deserializer, Serialize, Serializer};

use crate::stake::stake;
use crate::utils::{config_file, setup_logging};
use crate::validator::run_validator;

#[derive(Serialize, Deserialize, Debug)]
pub struct Config {
    pub rpc_url: String,
    pub ws_url: String,
    pub program_id: String,
    pub keypair: InnerKeypair,
    pub log_level: String,
}

#[derive(derive_more::From, derive_more::Into)]
pub struct InnerKeypair(Keypair);

impl Serialize for InnerKeypair {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        let bytes = &self.0.to_bytes()[..];
        serde_bytes::Bytes::new(bytes).serialize(serializer)
    }
}

impl<'d> Deserialize<'d> for InnerKeypair {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'d>,
    {
        let bytes = <serde_bytes::ByteBuf>::deserialize(deserializer)?;
        Keypair::from_bytes(bytes.as_ref())
            .map(Self)
            .map_err(SerdeError::custom)
    }
}

impl Display for InnerKeypair {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        Display::fmt(&self.0.pubkey(), f)
    }
}

impl Debug for InnerKeypair {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_tuple("PublicKey").field(&self.0.pubkey()).finish()
    }
}

#[derive(Parser, Debug)]
#[command(author, version, about, long_about = None)]
#[command(arg_required_else_help(true))]
#[command(next_line_help = true)]
struct Cli {
    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand, Debug)]
enum Commands {
    /// Command to run the validator once the config has been set.
    Run(RunArgs),
    /// Command to run the validator for the first time where the config is set.
    Init(InitArgs),
    /// Command to stake on the validator
    Stake(StakeArgs),
}

#[derive(Args, Debug)]
struct RunArgs {
    /// rpc url
    #[arg(short, long)]
    rpc_url: Option<String>,

    /// websocket url
    #[arg(short, long)]
    ws_url: Option<String>,

    /// program ID
    #[arg(long)]
    program_id: Option<String>,

    /// Private key
    #[arg(long)]
    keypair_path: Option<String>,

    /// Log Level
    #[arg(short, long)]
    log_level: Option<LevelFilter>,
}

#[derive(Args, Debug)]
struct InitArgs {
    /// rpc url
    #[arg(short, long)]
    rpc_url: String,

    /// websocket url
    #[arg(short, long)]
    ws_url: String,

    /// program ID
    #[arg(long)]
    program_id: String,

    /// Private key
    #[arg(long)]
    keypair_path: String,

    /// Log Level
    #[arg(short, long)]
    log_level: Option<LevelFilter>,
}

#[derive(Args, Debug)]
struct StakeArgs {
    /// Total amount to stake including decimals
    #[arg(short, long)]
    amount: u64,

    /// Mint of the token to be staked
    #[arg(short, long)]
    token_mint: String,

    /// rpc url
    #[arg(short, long)]
    rpc_url: Option<String>,

    /// websocket url
    #[arg(short, long)]
    ws_url: Option<String>,

    /// program ID
    #[arg(long)]
    program_id: Option<String>,

    /// genesis hash
    #[arg(short, long)]
    genesis_hash: Option<String>,

    /// Private key
    #[arg(long)]
    keypair_path: Option<String>,

    /// Log Level
    #[arg(short, long)]
    log_level: Option<LevelFilter>,
}

#[derive(Clone, Debug)]
pub enum Values {
    Yes,
    No,
}

impl FromStr for Values {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        if s.eq_ignore_ascii_case("yes") {
            Ok(Values::Yes)
        } else if s.eq_ignore_ascii_case("no") {
            Ok(Values::No)
        } else {
            Err(format!("Can not parse {}", s))
        }
    }
}

impl Display for Values {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        core::fmt::Debug::fmt(self, f)
    }
}

pub fn process_command() {
    let args = Cli::parse();
    match args.command {
        Commands::Run(cmd) => {
            let config_file = config_file();
            let config_data = fs::read_to_string(config_file).expect(
                "Failed to read config file; make sure you’ve run init \
                 command first.",
            );
            let default_config: Config = toml::from_str(&config_data).unwrap();
            let keypair = if let Some(keypair_path) = cmd.keypair_path {
                let keypair = read_keypair_file(keypair_path)
                    .expect("Unable to read keypair file");
                keypair.into()
            } else {
                default_config.keypair
            };
            let config = Config {
                rpc_url: cmd.rpc_url.unwrap_or(default_config.rpc_url),
                ws_url: cmd.ws_url.unwrap_or(default_config.ws_url),
                program_id: cmd.program_id.unwrap_or(default_config.program_id),
                keypair,
                log_level: cmd
                    .log_level
                    .unwrap_or(LevelFilter::Info)
                    .to_string(),
            };
            setup_logging(LevelFilter::from_str(&config.log_level).unwrap());
            run_validator(config)
        }
        Commands::Init(cmd) => {
            let config_file = config_file();
            setup_logging(cmd.log_level.unwrap_or(LevelFilter::Info));
            if config_file.exists() {
                let value = prompt(
                    "Do you really want to overwrite. Enter yes or no.",
                    Some(Values::No),
                );
                if matches!(value, Values::No) {
                    log::info!("Skipped overwriting the config file");
                }
                log::info!("Overwriting config file");
            }
            let keypair = read_keypair_file(&cmd.keypair_path)
                .expect("Unable to read keypair file");
            // let keypair = keypair.to_bytes().to_vec();
            let config = Config {
                rpc_url: cmd.rpc_url,
                ws_url: cmd.ws_url,
                program_id: cmd.program_id,
                keypair: keypair.into(),
                log_level: cmd
                    .log_level
                    .unwrap_or(LevelFilter::Info)
                    .to_string(),
            };
            let toml_in_string = toml::to_string(&config).unwrap();
            fs::write(config_file, toml_in_string).unwrap();
            log::info!("New Config {:?}", config);
        }
        Commands::Stake(cmd) => {
            let config_file = config_file();
            let config_data = fs::read_to_string(config_file).expect(
                "Failed to read config file; make sure you’ve run init \
                 command first.",
            );
            let default_config: Config = toml::from_str(&config_data).unwrap();
            let keypair = if let Some(keypair_path) = cmd.keypair_path {
                let keypair = read_keypair_file(keypair_path)
                    .expect("Unable to read keypair file");
                keypair.into()
            } else {
                default_config.keypair
            };
            let config = Config {
                rpc_url: cmd.rpc_url.unwrap_or(default_config.rpc_url),
                ws_url: cmd.ws_url.unwrap_or(default_config.ws_url),
                program_id: cmd.program_id.unwrap_or(default_config.program_id),
                genesis_hash: cmd
                    .genesis_hash
                    .unwrap_or(default_config.genesis_hash),
                keypair,
                log_level: cmd
                    .log_level
                    .unwrap_or(LevelFilter::Info)
                    .to_string(),
            };
            setup_logging(LevelFilter::from_str(&config.log_level).unwrap());
            let token_mint = Pubkey::from_str(&cmd.token_mint).unwrap();
            stake(config, cmd.amount, token_mint);
        }
    }
}

/// Prompt for user input with the ability to provide a default value
fn prompt<T>(prompt: &str, default: Option<T>) -> T
where
    T: Clone + ToString + FromStr,
    <T as FromStr>::Err: Debug + ToString,
{
    let theme = &ColorfulTheme::default();
    let mut builder = Input::<T>::with_theme(theme);
    builder.with_prompt(prompt);

    if let Some(default) = default {
        builder.default(default);
    }

    builder.interact_text().unwrap()
}
