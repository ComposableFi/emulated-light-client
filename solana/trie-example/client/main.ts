import {
	establishConnection,
	establishPayer,
	checkProgram,
	getKey,
	setKey,
	sealKey,
} from './trie';

async function main() {
	const operation = parseArgv(process.argv);
	if (!operation) {
		return;
	}

	// Establish connection to the cluster
	await establishConnection();

	// Determine who pays for the fees
	await establishPayer();

	// Check if the program has been deployed
	await checkProgram();

	switch (operation[0]) {
		case "get":
			await getKey(operation[1]);
			break;
		case "set":
			await setKey(operation[1], operation[2]);
			break;
		case "seal":
			await sealKey(operation[1]);
			break;
	}

	console.log('Success');
}

function parseArgv(argv: string[]): string[] | null {
	const cmd = argv[0] + ' ' + argv[1];
	switch (argv[2] || "--help") {
		case "get":
		case "seal":
			if (argv.length != 4) {
				break;
			}
			return [argv[2], argv[3]];
		case "set":
			if (argv.length != 5) {
				break;
			}
			return [argv[2], argv[3], argv[4]];
		case "help":
		case "--help":
		case "-h": {
			console.log(
				`usage: ${cmd} get <hex-key>\n` +
				`       ${cmd} set <hex-key> <hex-hash>\n` +
				`       ${cmd} seal <hex-key>`
			)
			process.exit(0);
		}
	}
	console.error(`Invalid usage; see ${cmd} --help`);
	process.exit(-1);
}

main().then(
	() => process.exit(),
	err => {
		console.error(err);
		process.exit(-1);
	},
);
