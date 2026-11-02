import test from "node:test";
import assert from "node:assert/strict";
import { registerHooks } from "node:module";
import { existsSync } from "node:fs";
import { fileURLToPath } from "node:url";
import { nativeOffer, unknownMachineOffer } from "./native-offers.fixture.mjs";

// Exercise the actual adapter without emitting JS, initializing i18n, or invoking a native/network operation.
registerHooks({
  resolve(specifier, context, nextResolve) {
    if (context.parentURL?.endsWith("/lib/backend.ts")) {
      if (specifier === "./tauri") return {
        url: `data:text/javascript,${encodeURIComponent("export function invokeSafe(command, args) { return globalThis.__offerInvoke(command, args); }")}`,
        shortCircuit: true,
      };
      if (specifier === "./i18n") return {
        url: `data:text/javascript,${encodeURIComponent("export const getActiveLocale = () => 'en'; export const translate = (key) => key;")}`,
        shortCircuit: true,
      };
    }
    if (specifier.startsWith(".") && context.parentURL?.startsWith("file:")) {
      const candidate = new URL(`${specifier}.ts`, context.parentURL);
      if (existsSync(fileURLToPath(candidate))) return { url: candidate.href, shortCircuit: true };
    }
    return nextResolve(specifier, context);
  },
});
const { searchOffers } = await import("../../../lib/backend.ts");

test("real search adapter preserves native string machine IDs without changing provider order or offer identity", async () => {
  const rows = [nativeOffer(), { ...nativeOffer(), id: 346 }, nativeOffer("9223372036854775807"), unknownMachineOffer];
  const calls = [];
  globalThis.__offerInvoke = async (command, args) => { calls.push([command, args]); return rows; };
  const result = await searchOffers(2, 24);
  assert.deepEqual(calls, [["search_offers", { page: 2, pageSize: 24 }]]);
  assert.deepEqual(result.map((offer) => offer.providerMachineId), ["9007199254740993", "9007199254740993", "9223372036854775807", null]);
  assert.deepEqual(result.map((offer) => offer.id), rows.map((offer) => offer.id));
  assert.deepEqual(result.map((offer) => [offer.reliability, offer.score, offer.hourlyPrice]), rows.map((offer) => [offer.reliability, offer.score, offer.hourlyPrice]));
  assert.equal("providerMachineId" in rows[0], false, "normalization must not mutate returned provider objects");
});

test("real adapter never stringifies unsafe or numeric machine IDs, joins a host, or accepts conflicting identities", async () => {
  globalThis.__offerInvoke = async () => [nativeOffer(9007199254740993), nativeOffer(123), nativeOffer("9223372036854775808"),
    { ...nativeOffer(), providerMachineId: "999" }, unknownMachineOffer];
  assert.deepEqual((await searchOffers()).map((offer) => offer.providerMachineId), [null, null, null, null, null]);
});
