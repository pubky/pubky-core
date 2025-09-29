// /**
//  * Request a signup token from the homeserver admin endpoint.
//  *
//  * @param {string} [homeserverAddress="localhost:6288"]
//  *   Host:port of the homeserver admin HTTP endpoint (testnet default).
//  * @param {string} [adminPassword="admin"]
//  *   Admin password sent as `X-Admin-Password`.
//  * @returns {Promise<string>} The signup token.
//  */
// export async function createSignupToken(
//   homeserverAddress = "localhost:6288",
//   adminPassword = "admin",
// ) {
//   const url = `http://${homeserverAddress}/generate_signup_token`;

//   const res = await fetch(url, {
//     method: "GET",
//     headers: { "X-Admin-Password": adminPassword },
//   });

//   const body = await res.text().catch(() => "");
//   if (!res.ok) {
//     throw new Error(
//       `Failed to get signup token: ${res.status} ${res.statusText}${
//         body ? ` - ${body}` : ""
//       }`,
//     );
//   }

//   return body;
// }
