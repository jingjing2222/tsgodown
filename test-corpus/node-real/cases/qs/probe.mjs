import qs from "qs";

const parsedNested = qs.parse(
  "user[name]=kim&user[roles][]=admin&user[roles][]=ops",
);
const parsedRepeated = qs.parse("tag=a&tag=b&tag=c");
const parsedEncoded = qs.parse("space=a%20b&reserved=%5Bvalue%5D");
const stringifiedNested = qs.stringify(
  { user: { name: "kim", roles: ["admin", "ops"] } },
  { encodeValuesOnly: true },
);
const stringifiedArray = qs.stringify(
  { tag: ["a", "b", "c"] },
  { arrayFormat: "repeat" },
);

const report = {
  package: "qs",
  probes: {
    parsedNested,
    parsedRepeated,
    parsedEncoded,
    stringifiedNested,
    stringifiedArray,
  },
};

console.log(JSON.stringify(report, null, 2));
