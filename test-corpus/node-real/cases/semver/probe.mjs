import semver from "../../packages/semver/index.js";

const versions = ["1.2.3", "1.2.3-beta.2", "2.0.0", "1.10.0", "bad"];

const report = {
  package: "semver",
  probes: {
    valid: versions.map((value) => [value, semver.valid(value)]),
    comparePrerelease: semver.compare("1.2.3-beta.2", "1.2.3"),
    ranges: [
      ["1.4.0", "^1.2.3", semver.satisfies("1.4.0", "^1.2.3")],
      ["2.0.0", "^1.2.3", semver.satisfies("2.0.0", "^1.2.3")],
      ["1.2.9", "~1.2.3", semver.satisfies("1.2.9", "~1.2.3")],
      ["1.3.0", "~1.2.3", semver.satisfies("1.3.0", "~1.2.3")],
      ["1.5.0", ">=1.0.0 <2", semver.satisfies("1.5.0", ">=1.0.0 <2")],
    ],
    sorted: semver.sort(["1.2.3", "1.2.3-beta.2", "2.0.0", "1.10.0"]),
    cliStyle: {
      input: ["1.2.3", "1.3.0", "2.0.0"],
      range: "^1.2.3",
      output: ["1.2.3", "1.3.0", "2.0.0"].filter((value) =>
        semver.satisfies(value, "^1.2.3"),
      ),
    },
  },
};

console.log(JSON.stringify(report, null, 2));
