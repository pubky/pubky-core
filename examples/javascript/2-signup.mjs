#!/usr/bin/env node
// Sign up using a recovery file -> print session info.
import { Pubky, Keypair, PublicKey } from "@synonymdev/pubky";
import { args, promptHidden, readFileUint8 } from "./_cli.mjs";

const usage = `
Usage:
  node 2-signup.mjs <homeserver_pubky> </path/to/recovery_file> [signup_code] [--testnet]

Example:
  node 2-signup.mjs pubky8pinxxg... ./alice.recovery INVITE-123 --testnet
`;

const a = args(process.argv.slice(2), { usage });
const [homeserverArg, recoveryPath, signupCode] = a._;
if (!homeserverArg || !recoveryPath) {
  console.error(usage.trim());
  process.exit(1);
}

// 1) Init a mainnet/testnet Pubky SDK entrypoint
const pubky = a.testnet ? Pubky.testnet() : new Pubky();

// 2) Decrypt recovery -> Keypair -> Signer
const passphrase = await promptHidden("Enter recovery passphrase: ");
const recoveryBytes = await readFileUint8(recoveryPath);
const keypair = Keypair.fromRecoveryFile(recoveryBytes, passphrase);
const signer = pubky.signer(keypair);

// 3) Signup at the homeserver (optional invite)
const homeserver = PublicKey.from(homeserverArg);
await signer.signup(homeserver, signupCode ?? null);

// 4) Sign in to create a grant-backed session for this example client
const session = await signer.signin("pubky-js-signup.example");

// 5) Show session owner + capabilities
console.log("\nSigned up as:", session.info.publicKey.toString());
console.log("Capabilities:", session.info.capabilities);
