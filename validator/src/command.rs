use std::fmt::{Debug, Display};
use std::fs;
use std::str::FromStr;

use anchor_client::solana_sdk::signature::{read_keypair_file, Keypair};
use anchor_client::solana_sdk::signer::Signer;
use clap::{arg, command, Args, Parser, Subcommand};
use dialoguer::theme::ColorfulTheme;
use dialoguer::Input;
use log::LevelFilter;
use serde::{Deserialize, Serialize};

use crate::utils::{config_file, setup_logging};

#[derive(Serialize, Deserialize, Debug)]
pub struct Config {
    pub rpc_url: String,
    pub ws_url: String,
    pub program_id: String,
    pub genesis_hash: String,
    pub keypair: Vec<u8>,
    pub log_level: String,
}

impl Display for Config {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let keypair = Keypair::from_bytes(&self.keypair).unwrap();
        write!(
            f,
            "\nrpc_url: {}\nws_url: {}\nprogram_id: {}\ngenesis_hash: \
             {}\nvalidator_public_key: {}\nlog_level: {}",
            self.rpc_url,
            self.ws_url,
            self.program_id,
            self.genesis_hash,
            keypair.pubkey().to_string(),
            self.log_level
        )
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

    /// genesis hash
    #[arg(short, long)]
    genesis_hash: String,

    /// Private key
    #[arg(long)]
    keypair_path: String,

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
            Err(format!("Can not parse {}", s).into())
        }
    }
}

impl Display for Values {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{:?}", self)
    }
}

pub fn parse_config() -> Option<Config> {
    let args = Cli::parse();
    match args.command {
        Commands::Run(cmd) => {
            let config_file = config_file();
            let config_data = fs::read_to_string(config_file).expect(
                "Failed to read config file, make sure u run init command \
                 before you try to run.",
            );
            let default_config: Config = toml::from_str(&config_data).unwrap();
            let keypair = if cmd.keypair_path.is_some() {
                let keypair = read_keypair_file(&cmd.keypair_path.unwrap())
                    .expect("Unable to read keypair file");
                keypair.to_bytes().to_vec()
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
            setup_logging(&config.log_level);
            Some(config)
        }
        Commands::Init(cmd) => {
            let config_file = config_file();
            let file = fs::metadata(config_file.clone());
            setup_logging(
                &cmd.log_level.unwrap_or(LevelFilter::Info).to_string(),
            );
            if file.is_ok() {
                let value = prompt(
                    "Do you really want to overwrite. Enter yes or no.",
                    Some(Values::No),
                );
                if matches!(value, Values::No) {
                    log::info!("Skipped overwriting the config file");
                    return None;
                }
                log::info!("Overwriting config file");
            }
            let keypair = read_keypair_file(&cmd.keypair_path)
                .expect("Unable to read keypair file");
            let keypair = keypair.to_bytes().to_vec();
            let config = Config {
                rpc_url: cmd.rpc_url,
                ws_url: cmd.ws_url,
                program_id: cmd.program_id,
                genesis_hash: cmd.genesis_hash,
                keypair,
                log_level: cmd
                    .log_level
                    .unwrap_or(LevelFilter::Info)
                    .to_string(),
            };
            let toml_in_string = toml::to_string(&config).unwrap();
            fs::write(config_file, toml_in_string).unwrap();
            log::info!("New Config {}", config);
            None
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
