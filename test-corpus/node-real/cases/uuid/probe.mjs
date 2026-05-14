import { parse, stringify, v4, v5, validate, version } from "uuid";

const fixed = "6fa459ea-ee8a-3ca4-894e-db77e160355e";
const random = v4();
const deterministic = v5("tsgodown", v5.URL);

const report = {
  package: "uuid",
  probes: {
    fixed,
    validateFixed: validate(fixed),
    versionFixed: version(fixed),
    roundTrip: stringify(parse(fixed)),
    deterministic,
    deterministicValid: validate(deterministic),
    randomShape:
      /^[0-9a-f]{8}-[0-9a-f]{4}-4[0-9a-f]{3}-[89ab][0-9a-f]{3}-[0-9a-f]{12}$/.test(
        random,
      ),
  },
};

console.log(JSON.stringify(report, null, 2));
