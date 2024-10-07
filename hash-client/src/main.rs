extern crate alloc;
extern crate core;

use std::process::ExitCode;

use solana_sdk::compute_budget::ComputeBudgetInstruction;
use solana_sdk::instruction::Instruction;
use solana_sdk::pubkey::Pubkey;
use solana_sdk::signature::Signer;
use solana_transaction_status::option_serializer::OptionSerializer;

mod args;

fn main() -> ExitCode {
    let opts = match args::parse(std::env::args()) {
        Ok(it) => it,
        Err(ec) => return ec,
    };
    match run(&opts) {
        Ok(ec) => ec,
        Err(err) => {
            eprintln!("{}: {}", opts.argv0, err);
            ExitCode::FAILURE
        }
    }
}


fn run(opts: &args::Opts) -> Result<ExitCode, Error> {
    println!("Connecting to {}...", opts.rpc_url);
    let client = solana_client::rpc_client::RpcClient::new(&opts.rpc_url);
    let blockhash = client.get_latest_blockhash()?;
    println!("Latest blockhash: {blockhash}");

    let mut instructions = Vec::with_capacity(4);
    if !opts.builtin {
        instructions
            .push(ComputeBudgetInstruction::set_compute_unit_limit(u32::MAX));
    }
    if opts.priority > 0 {
        instructions.push(ComputeBudgetInstruction::set_compute_unit_price(
            opts.priority,
        ));
    }

    let program_id = opts.program_id;
    let (accounts, data) = match opts.data {
        args::Data::Zeros(len) => {
            if len as usize > solana_sdk::entrypoint::HEAP_LENGTH {
                instructions
                    .push(ComputeBudgetInstruction::request_heap_frame(len));
            }
            let data =
                [&[opts.builtin as u8][..], &len.to_le_bytes()[..]].concat();
            (Vec::new(), data)
        }
        args::Data::Account(acc) => {
            (vec![acc_meta(acc)], vec![opts.builtin as u8])
        }
    };
    instructions.push(Instruction { program_id, accounts, data });

    let message = solana_sdk::message::Message::new_with_blockhash(
        &instructions[..],
        Some(&opts.keypair.pubkey()),
        &blockhash,
    );
    let mut tx = solana_sdk::transaction::Transaction::new_unsigned(message);

    // Simulate the transaction
    let program_id = opts.program_id.to_string();
    println!("Simulate transaction to {program_id}...");
    let result = client.simulate_transaction(&tx)?.value;
    if let Some(err) = result.err {
        for log in result.logs.as_deref().unwrap_or(&[][..]) {
            eprintln!("{}: {log}", opts.argv0);
        }
        eprintln!("{}: {err}", opts.argv0);
        return Ok(ExitCode::FAILURE);
    }

    // Send the transaction
    println!("Sending transaction to {program_id}...");
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

    Ok(ExitCode::SUCCESS)
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

fn acc_meta(pubkey: Pubkey) -> solana_sdk::instruction::AccountMeta {
    solana_sdk::instruction::AccountMeta {
        pubkey,
        is_signer: false,
        is_writable: false,
    }
}

#[derive(derive_more::From, derive_more::Display)]
enum Error {
    Msg(&'static str),
    Client(solana_client::client_error::ClientError),
}
