extern crate alloc;
extern crate std;

use std::process::ExitCode;

use base64::Engine;
use solana_sdk::pubkey::Pubkey;
use solana_sdk::signature::Signer;
use solana_transaction_status::option_serializer::OptionSerializer;
use wittrie::api;

mod args;

fn main() -> ExitCode {
    let opts = match args::parse(std::env::args()) {
        Ok(it) => it,
        Err(ec) => return ec,
    };
    run(&opts).unwrap_or_else(|err| err.eprint(&opts.argv0))
}


fn acc_meta(
    pubkey: Pubkey,
    is_signer: bool,
    is_writable: bool,
) -> solana_sdk::instruction::AccountMeta {
    solana_sdk::instruction::AccountMeta { pubkey, is_signer, is_writable }
}

fn as_deref<T>(opt: &OptionSerializer<T>) -> Option<&T::Target>
where
    T: core::ops::Deref,
{
    if let OptionSerializer::Some(ref v) = opt {
        Some(v.deref())
    } else {
        None
    }
}

fn run(opts: &args::Opts) -> Result<ExitCode, Error> {
    // Connect
    println!("Connecting to {}...", opts.rpc_url);
    let client = solana_client::rpc_client::RpcClient::new(&opts.rpc_url);
    let blockhash = client.get_latest_blockhash()?;
    println!("Latest blockhash: {blockhash}");

    // Construct the transaction
    println!("Root account: {}", opts.root_account);
    println!("Witness account: {}", opts.witness_account);
    let instruction = solana_sdk::instruction::Instruction {
        program_id: opts.program_id,
        accounts: vec![
            acc_meta(opts.keypair.pubkey(), true, true),
            acc_meta(opts.root_account, false, true),
            acc_meta(opts.witness_account, false, true),
            acc_meta(solana_sdk::system_program::ID, false, false),
        ],
        data: opts.data.to_vec(),
    };
    let message = solana_sdk::message::Message::new_with_blockhash(
        &[instruction],
        Some(&opts.keypair.pubkey()),
        &blockhash,
    );
    let mut tx = solana_sdk::transaction::Transaction::new_unsigned(message);

    // Simulate the transaction
    println!("Simulate transaction...");
    let result = client.simulate_transaction(&tx)?.value;
    if let Some(err) = result.err {
        for log in result.logs.as_deref().unwrap_or(&[][..]) {
            eprintln!("{}: {log}", opts.argv0);
        }
        eprintln!("{}: {err}", opts.argv0);
        return Ok(ExitCode::FAILURE);
    }

    // Send the transaction
    println!("Sending transaction...");
    let blockhash = client.get_latest_blockhash()?;
    println!("Latest blockhash: {blockhash}");
    tx.sign(&[&opts.keypair], blockhash);
    let sig = client.send_and_confirm_transaction(&tx)?;
    println!("Signature: {sig}");

    // Get the transaction
    let encoding = solana_transaction_status::UiTransactionEncoding::Binary;
    let resp = client.get_transaction(&sig, encoding)?;
    let (slot, tx) = (resp.slot, resp.transaction);
    println!("Executed in slot: {slot}");
    let meta =
        tx.meta.ok_or(Error::Msg("no transaction metadata in response"))?;
    for msg in as_deref(&meta.log_messages).unwrap_or(&[][..]) {
        println!("{msg}");
    }

    // Get the return data.
    let ret = match meta.return_data {
        OptionSerializer::Some(ret) => ret,
        _ => return Err("no return data from transaction".into()),
    };

    let program_id = opts.program_id.to_string();
    if ret.program_id != program_id {
        eprintln!(
            "{}: return data from {} rather than {}",
            opts.argv0, ret.program_id, program_id
        );
        eprintln!("{}: {:?}", opts.argv0, ret.data);
        return Ok(ExitCode::FAILURE);
    }
    if ret.data.1 != solana_transaction_status::UiReturnDataEncoding::Base64 {
        eprintln!(
            "{}: unrecognised return data encoding: {:?}",
            opts.argv0, ret.data.1
        );
        eprintln!("{}: {:?}", opts.argv0, ret.data);
        return Ok(ExitCode::FAILURE);
    };
    let data = base64::engine::general_purpose::STANDARD
        .decode(&ret.data.0)
        .map_err(|err| {
            eprintln!("{}: error decoding return data: {err}", opts.argv0);
            eprintln!("{}: return data: {}", opts.argv0, ret.data.0);
            Error::None
        })?;

    // Decode the data
    let data = bytemuck::from_bytes::<api::ReturnData>(&data);
    println!("Witness ({}):", opts.witness_account);
    println!("  lamports: {}", data.lamports());
    println!("  executable: {}", data.executable());
    println!("  rent_epoch: {}", data.rent_epoch());
    println!("  trie_hash: {}", hex::display(data.trie_hash()));
    println!("  slot: {}", data.slot());
    println!(
        "  hash: {}",
        hex::display(
            &data.hash_account(&opts.witness_account, &opts.program_id,)
        )
    );

    Ok(ExitCode::SUCCESS)
}

#[derive(derive_more::From, derive_more::Display)]
enum Error {
    None,
    Msg(&'static str),
    Client(solana_client::client_error::ClientError),
    B64Decode(base64::DecodeError),
}

impl Error {
    pub fn eprint(self, argv0: &str) -> ExitCode {
        if !matches!(self, Error::None) {
            eprintln!("{argv0}: {self}");
        }
        ExitCode::FAILURE
    }
}
