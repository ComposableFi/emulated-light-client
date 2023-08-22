/* eslint-disable @typescript-eslint/no-unsafe-assignment */
/* eslint-disable @typescript-eslint/no-unsafe-member-access */

import {
	Keypair,
	Connection,
	PublicKey,
	LAMPORTS_PER_SOL,
	SystemProgram,
	TransactionInstruction,
	Transaction,
	sendAndConfirmTransaction,
} from '@solana/web3.js';
import fs from 'mz/fs';
import path from 'path';

import {getPayer, getRpcUrl, createKeypairFromFile} from './utils';

/**
 * Connection to the network
 */
let connection: Connection;

/**
 * Keypair associated to the fees’ payer
 */
let payer: Keypair;

/**
 * Example trie’s program id
 */
let programId: PublicKey;

/**
 * The public key of the trie account.
 */
let triePubkey: PublicKey;

/**
 * Path to program files
 */
const PROGRAM_PATH = path.resolve(__dirname, '../../../dist/trie-example');

/**
 * Path to program shared object file which should be deployed on chain.
 */
const PROGRAM_SO_PATH = path.join(PROGRAM_PATH, 'trie.so');

/**
 * Path to the keypair of the deployed program.
 * This file is created when running `solana program deploy dist/trie-example/trie.so`
 */
const PROGRAM_KEYPAIR_PATH = path.join(PROGRAM_PATH, 'trie-keypair.json');

/**
 * The expected size of the account.
 */
const ACCOUNT_SIZE = 72000 + 64;

/**
 * Establish a connection to the cluster
 */
export async function establishConnection(): Promise<void> {
	const rpcUrl = await getRpcUrl();
	connection = new Connection(rpcUrl, 'confirmed');
	const version = await connection.getVersion();
	console.log('Connection to cluster established:', rpcUrl, version);
}

/**
 * Establish an account to pay for everything
 */
export async function establishPayer(): Promise<void> {
	let fees = 0;
	if (!payer) {
		const {feeCalculator} = await connection.getRecentBlockhash();

		// Calculate the cost to fund the trie account
		fees += await connection.getMinimumBalanceForRentExemption(ACCOUNT_SIZE);

		// Calculate the cost of sending transactions
		fees += feeCalculator.lamportsPerSignature * 100; // wag

		payer = await getPayer();
	}

	let lamports = await connection.getBalance(payer.publicKey);
	// if (lamports < fees) {
	// 	// If current balance is not enough to pay for fees, request an airdrop
	// 	const sig = await connection.requestAirdrop(
	// 		payer.publicKey,
	// 		fees - lamports,
	// 	);
	// 	await connection.confirmTransaction(sig);
	// 	lamports = await connection.getBalance(payer.publicKey);
	// }

	console.log(
		'Using account',
		payer.publicKey.toBase58(),
		'containing',
		lamports / LAMPORTS_PER_SOL,
		'SOL to pay for fees',
	);
}

/**
 * Check if the hello world BPF program has been deployed
 */
export async function checkProgram(): Promise<void> {
	// Read program id from keypair file
	try {
		const programKeypair = await createKeypairFromFile(PROGRAM_KEYPAIR_PATH);
		programId = programKeypair.publicKey;
	} catch (err) {
		const errMsg = (err as Error).message;
		throw new Error(
			`Failed to read program keypair at '${PROGRAM_KEYPAIR_PATH}' due to error: ${errMsg}. Program may need to be deployed with \`solana program deploy dist/trie-example/trie.so\``,
		);
	}

	// Check if the program has been deployed
	const programInfo = await connection.getAccountInfo(programId);
	if (programInfo === null) {
		if (fs.existsSync(PROGRAM_SO_PATH)) {
			throw new Error(
				'Program needs to be deployed with `solana program deploy dist/trie-example/trie.so`',
			);
		} else {
			throw new Error('Program needs to be built and deployed');
		}
	} else if (!programInfo.executable) {
		throw new Error(`Program is not executable`);
	}
	console.log(`Using program ${programId.toBase58()}`);

	// Derive the address (public key) of a trie account from the program so that it's easy to find later.
	const SEED = 'hello';
	triePubkey = await PublicKey.createWithSeed(
		payer.publicKey,
		SEED,
		programId,
	);

	// Check if the greeting account has already been created
	const trieAccount = await connection.getAccountInfo(triePubkey);
	if (trieAccount === null) {
		console.log(
			'Creating account',
			triePubkey.toBase58(),
			'to say hello to',
		);
		const lamports = await connection.getMinimumBalanceForRentExemption(
			ACCOUNT_SIZE,
		);

		const transaction = new Transaction().add(
			SystemProgram.createAccountWithSeed({
				fromPubkey: payer.publicKey,
				basePubkey: payer.publicKey,
				seed: SEED,
				newAccountPubkey: triePubkey,
				lamports,
				space: ACCOUNT_SIZE,
				programId,
			}),
		);
		await sendAndConfirmTransaction(connection, transaction, [payer]);
	}
}

/**
 * Get value of a key
 */
export async function getKey(hexKey: string): Promise<void> {
	console.log('Getting key %s from %s', hexKey, triePubkey.toBase58());
	await call('00' + hexKey);
}

/**
 * Get value of a key
 */
export async function setKey(hexKey: string, hexValue: string): Promise<void> {
	console.log('Setting key %s to %s in %s', hexKey, hexValue, triePubkey.toBase58());
	if (hexValue.length != 64) {
		throw new Error('Value must be 64 hexadecimal digits');
	}
	await call('02' + hexKey + hexValue);
}

/**
 * Get value of a key
 */
export async function sealKey(hexKey: string): Promise<void> {
	console.log('Sealing key %s in %s', hexKey, triePubkey.toBase58());
	await call('04' + hexKey);
}

/**
 * Sends an instruction to the contract.
 */
async function call(hexData: string): Promise<void> {
	let data = Buffer.from(hexData, 'hex');
	const instruction = new TransactionInstruction({
		keys: [{pubkey: triePubkey, isSigner: false, isWritable: true}],
		programId,
		data,
	});
	await sendAndConfirmTransaction(
		connection,
		new Transaction().add(instruction),
		[payer],
	);
}
