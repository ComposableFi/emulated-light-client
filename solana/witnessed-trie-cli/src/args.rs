use core::str::FromStr;

use solana_sdk::pubkey::{Pubkey, MAX_SEED_LEN};
use solana_sdk::signer::keypair::{read_keypair_file, Keypair};
use wittrie::api;

type RootSeed = arrayvec::ArrayVec<u8, { MAX_SEED_LEN }>;

/// Default program id to use if user doesn’t provide one.
const DEFAULT_PROGRAM_ID: Pubkey =
    solana_sdk::pubkey!("5QKjsHmVMQxccke2W58zuYoTCoySHauoisioYrbvVJxP");

/// Parse command line options.
pub struct Opts {
    /// The name this program was called with.
    pub argv0: String,

    /// Solana cluster’s RPC URL.
    pub rpc_url: String,

    /// Key pair to use when sending Solana transactions.
    pub keypair: Keypair,

    /// Witnessed trie program id.
    pub program_id: Pubkey,
    /// Trie account address.
    pub root_account: Pubkey,
    /// Trie witness address.
    pub witness_account: Pubkey,
    /// Instruction data to send to the witnessed trie program.
    pub data: api::OwnedData,
}

/// Prints usage information.
fn usage(argv0: &str, full: bool) {
    eprintln!("usage: {argv0} [<switch> ...] [<op> ...]");
    if !full {
        eprintln!("Use {argv0} --help for help");
        return;
    }

    #[rustfmt::skip]
    eprintln!(concat!(
        "<switch> is one of:\n",
        "    -r --rpc-url=<url>    RPC URL\n",
        "       --<cluster>        Use RPC for given <cluster> where <cluster> is one of:\n",
        "                          ‘localnet’, ‘devnet’, ‘testnet’, ‘mainnet-beta’\n",
        "    -k --keypair=<path>   Path to the keypair to use when sending transaction\n",
        "    -p --program-id=<id>  Id of the wittrie program\n",
        "    -s --seed=<seed>      Seed of the root trie PDA; empty by default\n",
        "    -b --bump=<bump>      Bump of the root trie PDA; calculated by default\n",
        "<op> is one of:\n",
        "     set  <key> <value>   Sets <key> to hash(<value>)\n",
        "     del  <key>           Deletes <key>\n",
        "     seal <key>           Seals <key>\n",
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
    let mut program_id = DEFAULT_PROGRAM_ID;
    let mut root_seed: RootSeed = Default::default();
    let mut bump = None;
    let mut ops = Vec::new();

    // Parse command line arguments
    while let Some(arg) = prog.next() {
        if arg == "-h" || arg == "--help" {
            return Err(true);
        } else if let Some(url) = parse_rpc_url(&arg) {
            rpc_url = Some(url);
        } else if let Some(url) =
            prog.parse_flag(&arg, "-u", "--rpc-url", |arg| {
                Result::<_, core::convert::Infallible>::Ok(arg.into())
            })?
        {
            rpc_url = Some(url);
        } else if let Some(pair) =
            prog.parse_flag(&arg, "-k", "--keypair", |path| {
                read_keypair_file(path)
            })?
        {
            keypair = Some(pair);
        } else if let Some(id) =
            prog.parse_flag(&arg, "-p", "--program-id", Pubkey::from_str)?
        {
            program_id = id;
        } else if let Some(value) =
            prog.parse_flag(&arg, "-s", "--seed", parse_seed)?
        {
            root_seed = value;
        } else if let Some(value) =
            prog.parse_flag(&arg, "-b", "--bump", u8::from_str)?
        {
            bump = Some(value);
        } else {
            ops.push(parse_op(prog, &arg).map_err(|err| {
                eprintln!("{prog}: {arg}: {err}");
                false
            })?);
        }
    }

    // Get Solana config.
    let mut config = None;
    let rpc_url =
        rpc_url.map_or_else(|| get_default_rpc_url(prog, &mut config), Ok)?;
    let keypair =
        keypair.map_or_else(|| get_default_keypair(prog, &mut config), Ok)?;

    // Get account addresses.
    let (root_account, root_bump) =
        api::find_root_account(&program_id, &root_seed, bump).ok_or_else(
            || {
                eprintln!("{prog}: unable to find trie root PDA");
                false
            },
        )?;
    let (witness_account, _) =
        api::find_witness_account(&program_id, &root_account).ok_or_else(
            || {
                eprintln!("{prog}: unable to find trie witness PDA");
                false
            },
        )?;

    // Get program's instruction data
    let data = api::OwnedData { root_seed, root_bump, ops };

    Ok(Opts {
        argv0: core::mem::take(&mut prog.argv0),
        rpc_url,
        keypair,
        program_id,
        root_account,
        witness_account,
        data,
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



/// Tries to interpret `arg` as `--<cluster>` flag.
///
/// If `arg` is a `--<cluster>` flag returns RPC URL for given Solana cluster.
/// Recognised clusters are `localnet`, `devnet`, `testnet` and `mainnet-beta`.
/// If `arg` doesn’t correspond to any of those flags, returns `None`.
fn parse_rpc_url(arg: &str) -> Option<String> {
    let arg = arg.strip_prefix("--")?;
    match arg {
        "localnet" => Some("http://127.0.0.1:8899".into()),
        "devnet" | "testnet" | "mainnet-beta" => {
            Some(format!("https://api.{arg}.solana.com"))
        }
        _ => None,
    }
}

/// Parse the value as a PDA seed.
///
/// For it to be a valid seed, it must be at most 32 bytes.  Longer values
/// result in an error.
fn parse_seed(seed: &str) -> Result<RootSeed, &'static str> {
    seed.as_bytes().try_into().map_err(|_| "can be at most 32 characters")
}


/// Parses a single operation from command line arguments.
fn parse_op(prog: &mut Prog, arg: &str) -> Result<api::OwnedOp, &'static str> {
    let kind = match arg {
        "set" => api::OpDiscriminants::Set,
        "del" => api::OpDiscriminants::Del,
        "seal" => api::OpDiscriminants::Seal,
        _ if arg.starts_with('-') => return Err("unknown switch"),
        _ => return Err("unknown operation"),
    };

    let key = prog.next().ok_or("missing <key>")?;
    if key.is_empty() || key.len() > 255 {
        return Err("<key> must be between 1 and 255 bytes");
    }
    let key = key.into_bytes();

    Ok(match kind {
        api::OpDiscriminants::Set => {
            let value = prog.next().ok_or("missing <value>")?;
            let value = lib::hash::CryptoHash::digest(value.as_bytes());
            api::OwnedOp::Set(key, value)
        }
        api::OpDiscriminants::Del => api::OwnedOp::Del(key),
        api::OpDiscriminants::Seal => api::OwnedOp::Seal(key),
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
