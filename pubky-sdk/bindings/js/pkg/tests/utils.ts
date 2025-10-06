/**
 * Request a signup token from the homeserver admin endpoint.
 *
 * @param {string} [homeserverAddress="localhost:6288"]
 *   Host:port of the homeserver admin HTTP endpoint (testnet default).
 * @param {string} [adminPassword="admin"]
 *   Admin password sent as `X-Admin-Password`.
 * @returns {Promise<string>} The signup token.
 */
import type { PubkyError } from "../index.js";
import type { Test } from "tape";

export type PubkyErrorInstance = Error & PubkyError;

export type Assert<T extends true> = T;
export type IsExact<A, B> =
  (<T>() => T extends A ? 1 : 2) extends <T>() => T extends B ? 1 : 2
    ? true
    : false;

export async function createSignupToken(
  homeserverAddress = "localhost:6288",
  adminPassword = "admin",
): Promise<string> {
  const url = `http://${homeserverAddress}/generate_signup_token`;

  const res = await fetch(url, {
    method: "GET",
    headers: { "X-Admin-Password": adminPassword },
  });

  const body = await res.text().catch(() => "");
  if (!res.ok) {
    throw new Error(
      `Failed to get signup token: ${res.status} ${res.statusText}${
        body ? ` - ${body}` : ""
      }`,
    );
  }

  return body;
}

export function assertPubkyError(
  t: Test,
  error: unknown,
  message = "expected a PubkyError instance",
): asserts error is PubkyErrorInstance {
  if (
    typeof error === "object" &&
    error !== null &&
    error instanceof Error &&
    "name" in error &&
    typeof (error as { name: unknown }).name === "string" &&
    "message" in error &&
    typeof (error as { message: unknown }).message === "string"
  ) {
    return;
  }

  t.fail(message);
  throw error;
}

export function sleep(ms: number): Promise<void> {
  return new Promise((resolve) => setTimeout(resolve, ms));
}

export function getStatusCode(error: PubkyError): number | undefined {
  if (
    typeof error.data === "object" &&
    error.data !== null &&
    "statusCode" in error.data
  ) {
    const status = (error.data as { statusCode?: unknown }).statusCode;
    if (typeof status === "number") {
      return status;
    }
  }

  return undefined;
}
