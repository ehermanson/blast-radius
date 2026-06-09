#!/usr/bin/env node
'use strict';

// Shim that locates the platform-specific blast-radius binary (shipped as an
// optionalDependency, esbuild-style) and execs it, propagating the exact exit
// code. Exit codes 0/1/2/64 are a documented CI contract — do not remap them.
//
// This shim never downloads anything; if the platform package is missing the
// install was incomplete or the platform is unsupported.

const { spawnSync } = require('child_process');
const fs = require('fs');

const SUPPORTED = [
  'blast-radius-cli-linux-x64       (Linux x64, glibc)',
  'blast-radius-cli-linux-arm64     (Linux arm64, glibc)',
  'blast-radius-cli-linux-x64-musl  (Linux x64, musl e.g. Alpine)',
  'blast-radius-cli-darwin-x64      (macOS x64)',
  'blast-radius-cli-darwin-arm64    (macOS arm64)',
  'blast-radius-cli-win32-x64       (Windows x64)',
];

function isMusl() {
  // glibc exposes its version in the process report; musl does not.
  try {
    const report = process.report && process.report.getReport();
    if (report && report.header) {
      return !report.header.glibcVersionRuntime;
    }
  } catch (err) {
    // fall through to the filesystem probe
  }
  try {
    return (
      fs.existsSync('/lib/ld-musl-x86_64.so.1') ||
      fs.existsSync('/lib/ld-musl-aarch64.so.1')
    );
  } catch (err) {
    return false;
  }
}

// Returns the candidate platform package names, most preferred first.
function candidatePackages(platform, arch) {
  if (platform === 'linux' && arch === 'x64') {
    // On musl systems prefer the musl build but fall back to glibc, and vice
    // versa (the musl build is statically linked and runs on glibc too).
    return isMusl()
      ? ['blast-radius-cli-linux-x64-musl', 'blast-radius-cli-linux-x64']
      : ['blast-radius-cli-linux-x64', 'blast-radius-cli-linux-x64-musl'];
  }
  if (platform === 'linux' && arch === 'arm64') {
    return ['blast-radius-cli-linux-arm64'];
  }
  if (platform === 'darwin' && arch === 'x64') {
    return ['blast-radius-cli-darwin-x64'];
  }
  if (platform === 'darwin' && arch === 'arm64') {
    return ['blast-radius-cli-darwin-arm64'];
  }
  if (platform === 'win32' && arch === 'x64') {
    return ['blast-radius-cli-win32-x64'];
  }
  return [];
}

function resolveBinary() {
  const platform = process.platform;
  const arch = process.arch;
  const binName = platform === 'win32' ? 'blast-radius.exe' : 'blast-radius';
  const candidates = candidatePackages(platform, arch);

  for (const pkg of candidates) {
    try {
      return require.resolve(`${pkg}/bin/${binName}`);
    } catch (err) {
      // try the next candidate
    }
  }

  const expected =
    candidates.length > 0
      ? `Expected the npm package "${candidates[0]}" to be installed, but it was not found.`
      : `Your platform (${platform}-${arch}) has no prebuilt blast-radius binary.`;

  console.error(
    [
      `blast-radius: could not find a prebuilt binary for ${platform}-${arch}.`,
      '',
      expected,
      '',
      candidates.length > 0
        ? 'It is normally installed automatically as an optionalDependency of'
        : 'Prebuilt binaries exist for these platforms (installed automatically as',
      candidates.length > 0
        ? '"blast-radius-cli". If it is missing, your package manager may have'
        : 'optionalDependencies of "blast-radius-cli"):'
      ,
      ...(candidates.length > 0
        ? [
            'skipped optional dependencies (e.g. --no-optional / --omit=optional),',
            'or the lockfile was created on a different platform. Try reinstalling',
            'with optional dependencies enabled.',
            '',
            'Prebuilt binaries exist for these platforms:',
          ]
        : []),
      ...SUPPORTED.map((line) => `  - ${line}`),
      '',
      'Alternatively, build from source with the Rust toolchain:',
      '  cargo install blast-radius',
      '',
      'More info: https://github.com/ehermanson/blast-radius',
    ].join('\n')
  );
  process.exit(1);
}

const binary = resolveBinary();
const result = spawnSync(binary, process.argv.slice(2), { stdio: 'inherit' });

if (result.error) {
  console.error(`blast-radius: failed to run ${binary}: ${result.error.message}`);
  process.exit(1);
}

if (result.signal) {
  // Re-raise the signal so callers observe the same termination the child did
  // (shells report this as exit code 128 + signal number).
  process.kill(process.pid, result.signal);
  // If the signal was non-fatal for this process, fall back to a non-zero exit.
  process.exit(1);
}

process.exit(result.status === null ? 1 : result.status);
