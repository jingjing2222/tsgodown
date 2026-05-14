import { execa } from "../../packages/execa/index.js";

const ok = await execa(
  process.execPath,
  [
    "-e",
    "console.log(process.env.TSGODOWN_PROBE + ':' + process.argv[1])",
    "argv-value",
  ],
  {
    env: { TSGODOWN_PROBE: "ok" },
  },
);

let failed;
try {
  await execa(process.execPath, [
    "-e",
    "console.error('bad stderr'); process.exit(7)",
  ]);
} catch (error) {
  failed = {
    exitCode: error.exitCode,
    stdout: error.stdout,
    stderr: error.stderr,
    shortMessagePrefix: String(error.shortMessage).split("\n")[0],
  };
}

const report = {
  package: "execa",
  probes: {
    ok: {
      exitCode: ok.exitCode,
      stdout: ok.stdout,
      stderr: ok.stderr,
    },
    failed,
  },
};

console.log(JSON.stringify(report, null, 2));
