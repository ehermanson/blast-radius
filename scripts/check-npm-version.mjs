#!/usr/bin/env node
import { readFileSync } from "node:fs";
import process from "node:process";

const cargo = readFileSync("Cargo.toml", "utf8");
const cargoVersion = cargo.match(/^version\s*=\s*"([^"]+)"/m)?.[1];
if (!cargoVersion) {
  console.error("could not find package version in Cargo.toml");
  process.exit(1);
}

const wrapper = JSON.parse(
  readFileSync("npm/blast-radius-cli/package.json", "utf8"),
);

const failures = [];
if (wrapper.version !== cargoVersion) {
  failures.push(
    `wrapper version ${wrapper.version} does not match Cargo.toml ${cargoVersion}`,
  );
}

for (const [name, version] of Object.entries(wrapper.optionalDependencies || {})) {
  if (version !== cargoVersion) {
    failures.push(
      `${name} optionalDependency pins ${version}, expected ${cargoVersion}`,
    );
  }
}

if (failures.length > 0) {
  console.error(["npm version drift:", ...failures.map((line) => `  - ${line}`)].join("\n"));
  process.exit(1);
}

console.log(`npm wrapper version matches Cargo.toml (${cargoVersion})`);
