#!/usr/bin/env node
// Generates the per-platform npm packages for a blast-radius release.
//
// Usage:
//   node npm/build-platform-packages.mjs --version X.Y.Z --artifacts-dir DIR
//
// DIR must contain the extracted per-target binaries laid out as:
//   DIR/{target}/blast-radius        (unix targets)
//   DIR/{target}/blast-radius.exe    (windows targets)
//
// For each target this emits a ready-to-publish package directory under
// npm/dist/{package-name}/, and rewrites npm/blast-radius-cli/package.json's
// version and optionalDependencies pins to --version (in place).
//
// Plain Node (>= 18), no dependencies.

import fs from 'node:fs';
import path from 'node:path';
import process from 'node:process';
import { fileURLToPath } from 'node:url';

const NPM_DIR = path.dirname(fileURLToPath(import.meta.url));
const DIST_DIR = path.join(NPM_DIR, 'dist');
const WRAPPER_MANIFEST = path.join(NPM_DIR, 'blast-radius-cli', 'package.json');

const DESCRIPTION_PREFIX =
  'Prebuilt blast-radius binary — analyze the transitive blast radius of code changes';
const REPOSITORY = {
  type: 'git',
  url: 'git+https://github.com/ehermanson/blast-radius.git',
};

// target triple -> npm platform package definition
const TARGETS = [
  {
    target: 'x86_64-unknown-linux-gnu',
    name: 'blast-radius-cli-linux-x64',
    os: 'linux',
    cpu: 'x64',
    libc: 'glibc',
    exe: false,
  },
  {
    target: 'aarch64-unknown-linux-gnu',
    name: 'blast-radius-cli-linux-arm64',
    os: 'linux',
    cpu: 'arm64',
    libc: 'glibc',
    exe: false,
  },
  {
    target: 'x86_64-unknown-linux-musl',
    name: 'blast-radius-cli-linux-x64-musl',
    os: 'linux',
    cpu: 'x64',
    libc: 'musl',
    exe: false,
  },
  {
    target: 'x86_64-apple-darwin',
    name: 'blast-radius-cli-darwin-x64',
    os: 'darwin',
    cpu: 'x64',
    libc: null,
    exe: false,
  },
  {
    target: 'aarch64-apple-darwin',
    name: 'blast-radius-cli-darwin-arm64',
    os: 'darwin',
    cpu: 'arm64',
    libc: null,
    exe: false,
  },
  {
    target: 'x86_64-pc-windows-msvc',
    name: 'blast-radius-cli-win32-x64',
    os: 'win32',
    cpu: 'x64',
    libc: null,
    exe: true,
  },
];

function fail(message) {
  console.error(`error: ${message}`);
  process.exit(1);
}

function parseArgs(argv) {
  const args = { version: null, artifactsDir: null };
  for (let i = 0; i < argv.length; i++) {
    const arg = argv[i];
    if (arg === '--version') {
      args.version = argv[++i];
    } else if (arg.startsWith('--version=')) {
      args.version = arg.slice('--version='.length);
    } else if (arg === '--artifacts-dir') {
      args.artifactsDir = argv[++i];
    } else if (arg.startsWith('--artifacts-dir=')) {
      args.artifactsDir = arg.slice('--artifacts-dir='.length);
    } else {
      fail(`unknown argument: ${arg}\nusage: node npm/build-platform-packages.mjs --version X.Y.Z --artifacts-dir DIR`);
    }
  }
  if (!args.version) fail('--version is required (e.g. --version 0.2.0)');
  if (!/^\d+\.\d+\.\d+(-[0-9A-Za-z.-]+)?$/.test(args.version)) {
    fail(`--version must be a semver version, got "${args.version}"`);
  }
  if (!args.artifactsDir) fail('--artifacts-dir is required');
  return args;
}

function checkArtifacts(artifactsDir) {
  if (!fs.existsSync(artifactsDir) || !fs.statSync(artifactsDir).isDirectory()) {
    fail(`artifacts dir not found: ${artifactsDir}`);
  }
  const missing = [];
  for (const t of TARGETS) {
    const bin = path.join(
      artifactsDir,
      t.target,
      t.exe ? 'blast-radius.exe' : 'blast-radius'
    );
    if (!fs.existsSync(bin)) missing.push(bin);
  }
  if (missing.length > 0) {
    const found = fs.readdirSync(artifactsDir);
    fail(
      [
        'missing release binaries:',
        ...missing.map((m) => `  - ${m}`),
        '',
        `contents of ${artifactsDir}:`,
        ...(found.length > 0 ? found.map((f) => `  - ${f}`) : ['  (empty)']),
      ].join('\n')
    );
  }
}

function buildPlatformPackage(t, version, artifactsDir) {
  const pkgDir = path.join(DIST_DIR, t.name);
  const binDir = path.join(pkgDir, 'bin');
  fs.rmSync(pkgDir, { recursive: true, force: true });
  fs.mkdirSync(binDir, { recursive: true });

  const manifest = {
    name: t.name,
    version,
    description: `${DESCRIPTION_PREFIX} (${t.os}-${t.cpu}${t.libc ? `, ${t.libc}` : ''})`,
    repository: REPOSITORY,
    license: 'MIT',
    preferUnplugged: true,
    files: ['bin'],
    os: [t.os],
    cpu: [t.cpu],
  };
  if (t.libc) manifest.libc = [t.libc];

  fs.writeFileSync(
    path.join(pkgDir, 'package.json'),
    JSON.stringify(manifest, null, 2) + '\n'
  );

  const binName = t.exe ? 'blast-radius.exe' : 'blast-radius';
  const src = path.join(artifactsDir, t.target, binName);
  const dest = path.join(binDir, binName);
  fs.copyFileSync(src, dest);
  if (!t.exe) fs.chmodSync(dest, 0o755);

  return { name: t.name, dir: pkgDir, binary: dest };
}

function rewriteWrapperManifest(version) {
  const manifest = JSON.parse(fs.readFileSync(WRAPPER_MANIFEST, 'utf8'));
  manifest.version = version;
  manifest.optionalDependencies = {};
  for (const t of TARGETS) {
    manifest.optionalDependencies[t.name] = version;
  }
  fs.writeFileSync(WRAPPER_MANIFEST, JSON.stringify(manifest, null, 2) + '\n');
}

function main() {
  const { version, artifactsDir } = parseArgs(process.argv.slice(2));
  const resolvedArtifacts = path.resolve(artifactsDir);

  checkArtifacts(resolvedArtifacts);

  const built = TARGETS.map((t) =>
    buildPlatformPackage(t, version, resolvedArtifacts)
  );
  rewriteWrapperManifest(version);

  console.log(`blast-radius npm packages for v${version}:`);
  for (const b of built) {
    const size = fs.statSync(b.binary).size;
    console.log(`  ${b.name}  ->  ${path.relative(process.cwd(), b.dir)}  (binary ${size} bytes)`);
  }
  console.log(
    `  blast-radius-cli  ->  ${path.relative(process.cwd(), path.dirname(WRAPPER_MANIFEST))}  (version + optionalDependencies pinned to ${version})`
  );
}

main();
