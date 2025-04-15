
/**
 * Util to request a signup token from the given homeserver as admin.
 *
 * @param {Client} client - An instance of your client.
 * @param {string} homeserver_address - The homeserver's public key (as a domain-like string).
 * @param {string} [adminPassword="admin"] - The admin password (defaults to "admin").
 * @returns {Promise<string>} - The signup token.
 * @throws Will throw an error if the request fails.
 */
export async function createSignupToken(client, homeserver_address ="localhost:6288", adminPassword = "admin") {
  const adminUrl = `http://${homeserver_address}/admin/generate_signup_token`;
  const response = await client.fetch(adminUrl, {
    method: "GET",
    headers: {
      "X-Admin-Password": adminPassword,
    },
  });

  if (!response.ok) {
    throw new Error(`Failed to get signup token: ${response.statusText}`);
  }

  return response.text();
}