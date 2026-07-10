#!/usr/bin/env node
// Approve a Pubky Auth URL (QR/deeplink) using a recovery file.
// If --testnet, we first ensure the user exists by signing up locally.
import { Pubky, Keypair, PublicKey } from "@synonymdev/pubky";
import { args, promptHidden, readFileUint8 } from "./_cli.mjs";

const TESTNET_HOMESERVER = "8pinxxgqs41n4aididenw5apqp1urfmzdztr8jt4abrkdn435ewo";
const DEFAULT_RECOVERY_FILE = new URL("../sample_recovery.key", import.meta.url);

const usage = `
Usage:
  node 2-authenticator.mjs "<AUTH_URL>" [--recovery-file <path>] [--testnet]

Examples:
  node 2-authenticator.mjs "pubkyauth:///?caps=/pub/my-cool-app/:rw&secret=...&relay=http://localhost:15412/inbox" --testnet
  node 2-authenticator.mjs "<AUTH_URL>" --testnet --recovery-file ./alice.recovery

You can try this out with the example backend-less third party browser application in /examples/rust/2-auth_flow/3rd-party-app
`;

const a = args(process.argv.slice(2), { usage });
const [authUrl] = a._;
if (!authUrl) {
  console.error(usage.trim());
  process.exit(1);
}

// 1) Choose facade (mainnet or testnet)
const pubky = a.testnet ? Pubky.testnet() : new Pubky();

// 2) Decrypt recovery -> Signer
const recoveryPath = a["recovery-file"] ?? DEFAULT_RECOVERY_FILE;
const recoveryBytes = await readFileUint8(recoveryPath);
let keypair;
try {
  keypair = Keypair.fromRecoveryFile(recoveryBytes, "");
} catch {
  const passphrase = await promptHidden("Enter recovery passphrase: ");
  keypair = Keypair.fromRecoveryFile(recoveryBytes, passphrase);
}
const signer = pubky.signer(keypair);

// 3) If testnet, ensure we exist (signup once)
if (a.testnet) {
  const homeserver = PublicKey.from(TESTNET_HOMESERVER);
  try {
    await signer.signup(homeserver);
    console.log("Signed up to the testnet homeserver.");
  } catch {
    console.log("Testnet user already exists, continuing...");
  }
}

// 4) Approve the auth request URL
await signer.approveAuthRequest(authUrl);
console.log(
  "Auth token delivered to relay. The 3rd-party app should have a valid Session or AuthToken now.",
);
