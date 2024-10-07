use core::str::FromStr;

use solana_sdk::pubkey::Pubkey;
use solana_sdk::signer::keypair::{read_keypair_file, Keypair};

/// Parse command line options.
pub struct Opts {
    /// The name this program was called with.
    pub argv0: String,

    /// Solana cluster’s RPC URL.
    pub rpc_url: String,

    /// Key pair to use when sending Solana transactions.
    pub keypair: Keypair,
    /// Priority fee.
    pub priority: u64,

    /// The test program’s Id.
    pub program_id: Pubkey,
    /// Whether to use native Ed25519 program.
    pub native: bool,
    /// Message to sign.
    pub message: String,
}

/// Prints usage information.
fn usage(argv0: &str, full: bool) {
    eprintln!("usage: {argv0} [<switch> ...] <message>");
    if !full {
        eprintln!("Use {argv0} --help for help");
        return;
    }

    #[rustfmt::skip]
    eprintln!(concat!(
        "<switch> is one of:\n",
        "    -p --program-id=<id>  Id of the Ed25519 test program\n",
        "    -n --native           Use Ed25519 native program\n",
        "\n",
        "    -u --rpc-url=<url>    RPC URL or cluster name which can be one of:\n",
        "                          ‘localnet’, ‘devnet’, ‘testnet’, ‘mainnet-beta’\n",
        "    -k --keypair=<path>   Path to the keypair to use when sending transaction\n",
        "    -P --priority=<fee>   Priority fee in micro lamports per Compute Unit\n",
    ));
}

/// Parses the command line options.
pub fn parse(args: std::env::Args) -> Result<Opts, std::process::ExitCode> {
    use std::process::ExitCode;
    let mut prog = Prog::new(args).ok_or_else(|| {
        usage("cli", true);
        ExitCode::FAILURE
    })?;
    parse_impl(&mut prog).map_err(|success| {
        usage(&prog.argv0, success);
        if success {
            ExitCode::SUCCESS
        } else {
            ExitCode::FAILURE
        }
    })
}

fn parse_impl(prog: &mut Prog) -> Result<Opts, bool> {
    let mut rpc_url = None;
    let mut keypair = None;
    let mut priority = 0;
    let mut native = false;
    let mut program_id = None;
    let mut message = None;

    // Parse command line arguments
    while let Some(arg) = prog.next() {
        if arg == "-h" || arg == "--help" {
            return Err(true);
        } else if let Some(url) =
            prog.parse_flag(&arg, "-u", "--rpc-url", parse_rpc_url)?
        {
            rpc_url = Some(url);
        } else if let Some(pair) =
            prog.parse_flag(&arg, "-k", "--keypair", |path| {
                read_keypair_file(path)
            })?
        {
            keypair = Some(pair);
        } else if let Some(pri) =
            prog.parse_flag(&arg, "-P", "--priority", |priority| {
                u64::from_str(priority)
            })?
        {
            priority = pri;
        } else if let Some(id) =
            prog.parse_flag(&arg, "-p", "--program-id", Pubkey::from_str)?
        {
            program_id = Some(id);
        } else if arg == "-n" || arg == "--native" {
            native = true;
        } else if arg.starts_with('-') {
            eprintln!("{prog}: {arg}: unknown switch");
            return Err(false);
        } else if message.is_some() {
            eprintln!("{prog}: message already provided");
            return Err(false);
        } else {
            message = Some(arg);
        }
    }

    let program_id = program_id.ok_or_else(|| {
        eprintln!("{prog}: missing --program-id");
        false
    })?;
    let message = message.ok_or_else(|| {
        eprintln!("{prog}: missing message");
        false
    })?;

    // Get Solana config.
    let mut config = None;
    let rpc_url =
        rpc_url.map_or_else(|| get_default_rpc_url(prog, &mut config), Ok)?;
    let keypair =
        keypair.map_or_else(|| get_default_keypair(prog, &mut config), Ok)?;

    Ok(Opts {
        argv0: core::mem::take(&mut prog.argv0),
        rpc_url,
        keypair,
        priority,
        program_id,
        native,
        message,
    })
}

/// Helper object which wraps argv0 and prog iterator together.
struct Prog {
    argv0: String,
    prog: std::env::Args,
}

impl Prog {
    fn new(mut prog: std::env::Args) -> Option<Self> {
        let argv0 = prog.next()?;
        Some(Self { argv0, prog })
    }

    fn next(&mut self) -> Option<String> { self.prog.next() }

    /// Checks whether argument matches given option and parses it if so.
    ///
    /// If the argument doesn’t match specified `short` or `long` option,
    /// returns `Ok(None)`.
    ///
    /// Otherwise, tries to parse the value using provided `parser` callback.
    /// If the callback succeeds returns `Ok(Some(value))` (where `value` is
    /// `Ok` value returned by the parser).  If the callback fails, returns
    /// `Err(false)`.
    fn parse_flag<T, E: core::fmt::Display>(
        &mut self,
        arg: &str,
        short: &str,
        long: &str,
        parser: impl FnOnce(&str) -> Result<T, E>,
    ) -> Result<Option<T>, bool> {
        let mut next = || {
            self.next().map(alloc::borrow::Cow::Owned).ok_or_else(|| {
                eprintln!("{self}: {arg} requires a value");
                false
            })
        };

        let value = if let Some(value) = arg.strip_prefix(short) {
            // ‘-f <value>’ or ‘-f<value>’
            if value.is_empty() {
                next()?
            } else {
                value.into()
            }
        } else if let Some(rest) = arg.strip_prefix(long) {
            // ‘--flag <value>’, ‘--flag=<value>’ or ‘--flagnot’
            if rest.is_empty() {
                next()?
            } else if let Some(value) = rest.strip_prefix('=') {
                value.into()
            } else {
                return Ok(None);
            }
        } else {
            return Ok(None);
        };

        match parser(&value) {
            Ok(value) => Ok(Some(value)),
            Err(err) => {
                eprintln!("{self}: {value}: {err}");
                Err(false)
            }
        }
    }
}

impl core::fmt::Display for Prog {
    fn fmt(&self, fmtr: &mut core::fmt::Formatter) -> core::fmt::Result {
        self.argv0.fmt(fmtr)
    }
}


/// Parses RPC URL argument.
///
/// If `arg` is one of the recognised clusters names (`localnet`, `devnet`,
/// `testnet` and `mainnet-beta`) returns URL for that cluster.  Otherwise,
/// returns `arg` unmodified.  Parsing never fails.
fn parse_rpc_url(arg: &str) -> Result<String, core::convert::Infallible> {
    Ok(match arg {
        "localnet" => "http://127.0.0.1:8899".into(),
        "devnet" | "testnet" | "mainnet-beta" => {
            format!("https://api.{arg}.solana.com")
        }
        arg => arg.into(),
    })
}

/// Loads Solana CLI configuration file if not already loaded.
fn load_solana_config<'a>(
    prog: &mut Prog,
    config: &'a mut Option<solana_cli_config::Config>,
) -> Result<&'a mut solana_cli_config::Config, bool> {
    if let Some(ref mut config) = config {
        return Ok(config);
    }
    let path = solana_cli_config::CONFIG_FILE.as_ref().ok_or_else(|| {
        eprintln!("{prog}: unable to find Solana CLI config file");
        false
    })?;
    let cfg = solana_cli_config::Config::load(path).map_err(|err| {
        eprintln!("{prog}: {path}: {err}");
        false
    })?;
    Ok(config.insert(cfg))
}

fn get_default_rpc_url(
    prog: &mut Prog,
    config: &mut Option<solana_cli_config::Config>,
) -> Result<String, bool> {
    let config = load_solana_config(prog, config)?;
    Ok(core::mem::take(&mut config.json_rpc_url))
}

fn get_default_keypair(
    prog: &mut Prog,
    config: &mut Option<solana_cli_config::Config>,
) -> Result<Keypair, bool> {
    let config = load_solana_config(prog, config)?;
    let path = &config.keypair_path;
    read_keypair_file(path).map_err(|err| {
        eprintln!("{prog}: {path}: {err}");
        false
    })
}
