import { LRUCache } from "lru-cache";

const cache = new LRUCache({ max: 2 });
cache.set("a", 1);
cache.set("b", 2);
const beforeGet = [...cache.entries()];
const getA = cache.get("a");
cache.set("c", 3);
const afterEvict = [...cache.entries()];

const ttlCache = new LRUCache({ max: 2, ttl: 50 });
ttlCache.set("short", "alive");
const ttlImmediate = ttlCache.get("short");
await new Promise((resolve) => setTimeout(resolve, 80));
const ttlExpired = ttlCache.get("short") ?? null;

const report = {
  package: "lru-cache",
  probes: {
    beforeGet,
    getA,
    afterEvict,
    hasA: cache.has("a"),
    hasB: cache.has("b"),
    hasC: cache.has("c"),
    ttlImmediate,
    ttlExpired,
  },
};

console.log(JSON.stringify(report, null, 2));
