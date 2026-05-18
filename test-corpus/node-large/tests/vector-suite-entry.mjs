#!/usr/bin/env node

import fs from "node:fs";
import { runVectorCase } from "./vector-runner.mjs";

const [, , corpus, vectorPath] = process.argv;

if (!corpus || !vectorPath) {
  console.error("usage: node vector-suite-entry.mjs <corpus> <vectors.json>");
  process.exit(2);
}

const vectors = JSON.parse(fs.readFileSync(vectorPath, "utf8"));
const results = [];

for (const vector of vectors.cases ?? []) {
  results.push({
    id: vector.id,
    result: await runVectorCase(corpus, vector),
  });
}

console.log(
  JSON.stringify({
    version: "node-large-vector-suite-result.v1",
    corpus,
    total: results.length,
    results,
  }),
);
