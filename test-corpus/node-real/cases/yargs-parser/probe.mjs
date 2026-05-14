import parser from "../../packages/yargs-parser/build/lib/index.js";

const argv = [
  "--name",
  "kim",
  "-abc",
  "--count",
  "3",
  "--tag",
  "red",
  "--tag",
  "green",
  "--no-cache",
  "pos1",
  "--",
  "--literal",
];

const parsed = parser(argv, {
  alias: { name: ["n"] },
  array: ["tag"],
  boolean: ["cache", "a", "b", "c"],
  configuration: {
    "populate--": true,
    "strip-aliased": true,
  },
  number: ["count"],
});

const report = {
  package: "yargs-parser",
  probes: {
    argv,
    parsed,
  },
};

console.log(JSON.stringify(report, null, 2));
