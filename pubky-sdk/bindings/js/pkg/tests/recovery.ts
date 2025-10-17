import test from "tape";

import { Keypair } from "../index.js";

test("recovery", async (t) => {
  const keypair = Keypair.random();

  const recoveryFile = keypair.createRecoveryFile("very secure password");

  t.is(recoveryFile.length, 91);
  t.deepEqual(
    Array.from(recoveryFile.slice(0, 19)),
    [
      112, 117, 98, 107, 121, 46, 111, 114, 103, 47, 114, 101, 99, 111, 118,
      101, 114, 121, 10,
    ],
  );

  const recovered = Keypair.fromRecoveryFile(
    recoveryFile,
    "very secure password",
  );

  t.is(recovered.publicKey.z32(), keypair.publicKey.z32());
});
