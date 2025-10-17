#!/usr/bin/env node
// Approve a Pubky Auth URL (QR/deeplink) using a recovery file.
// If --testnet, we first ensure the user exists by signing up locally.
import { Pubky, Keypair, PublicKey } from "@synonymdev/pubky";
import { args, promptHidden, readFileUint8 } from "./_cli.mjs";

const usage = `
Usage:
  npm run authenticator -- </path/to/recovery_file> "<AUTH_URL>" [--testnet] [--homeserver <pk>]

Example:
  npm run authenticator -- ./alice.pkarr "pubkyauth:///?caps=/pub/my-cool-app/:rw&secret=...&relay=http://localhost:15412/link" --testnet

You can try this out with the example backend-less third party browser application in /examples/rust/3-auth_flow/3rd-party-app
`;

const a = args(process.argv.slice(2), {
  usage,
  defaults: {
    homeserver: "8pinxxgqs41n4aididenw5apqp1urfmzdztr8jt4abrkdn435ewo",
  },
});
const [recoveryPath, authUrl] = a._;
if (!recoveryPath || !authUrl) {
  console.error(usage.trim());
  process.exit(1);
}

// 1) Choose facade (mainnet or testnet)
const pubky = a.testnet ? Pubky.testnet() : new Pubky();

// 2) Decrypt recovery -> Signer
const passphrase = await promptHidden("Enter recovery passphrase: ");
const recoveryBytes = await readFileUint8(recoveryPath);
const keypair = Keypair.fromRecoveryFile(recoveryBytes, passphrase);
const signer = pubky.signer(keypair);

// 3) If testnet, ensure we exist (signup once)
if (a.testnet) {
  const homeserver = PublicKey.from(a.homeserver);
  try {
    await signer.signup(homeserver, null);
    console.log("Testnet user signed up!");
  } catch {
    console.log("Testnet user was already signed up 👌");
  }
}

// 4) Approve the auth request URL
await signer.approveAuthRequest(authUrl);
console.log(
  "Auth token delivered to relay. The 3rd-party app should have a valid Session or AuthToken now.",
);
