#!/usr/bin/env node
// Accuracy oracle: differential test of blast-radius's import graph against
// dependency-cruiser (a mature, enhanced-resolve-based reference) on a fixture.
//
// It extracts the forward import-edge set (importer -> importee, internal files
// only) from each tool and reports the symmetric difference, classified so a
// human can tell a real blast-radius miss from a known/justified divergence.
//
//   node scripts/accuracy/oracle.mjs <fixture-dir> [--tsconfig <path>] [--json]
//
// Exit code is 0 unless --strict is passed and there are unexplained
// disagreements, so CI can gate on it later.

import { execFileSync } from 'node:child_process';
import { readdirSync, existsSync, statSync } from 'node:fs';
import path from 'node:path';
import { fileURLToPath } from 'node:url';

const HERE = path.dirname(fileURLToPath(import.meta.url));
const REPO_ROOT = path.resolve(HERE, '..', '..');
const DC_CONFIG = path.join(HERE, '.dependency-cruiser.cjs');
// Pinned exactly so the gate is deterministic — a depcruise patch must not be
// able to flip the result. Bump deliberately.
const DC_VERSION = 'dependency-cruiser@16.10.4';
const SOURCE_EXTS = new Set([
  '.ts', '.tsx', '.js', '.jsx', '.mjs', '.cjs', '.mts', '.cts',
]);
const IGNORED_DIRS = new Set([
  'node_modules', '.git', 'dist', 'build', 'coverage', '.next', '.turbo',
]);

function parseArgs(argv) {
  const args = { fixture: null, tsconfig: null, json: false, strict: false };
  for (let i = 0; i < argv.length; i++) {
    const a = argv[i];
    if (a === '--tsconfig') args.tsconfig = argv[++i];
    else if (a === '--json') args.json = true;
    else if (a === '--strict') args.strict = true;
    else if (!args.fixture) args.fixture = a;
  }
  if (!args.fixture) {
    console.error('usage: oracle.mjs <fixture-dir> [--tsconfig <path>] [--json] [--strict]');
    process.exit(64);
  }
  args.fixture = path.resolve(args.fixture);
  // dependency-cruiser runs with cwd = fixture, so the tsconfig must be an
  // absolute path regardless of where the oracle was invoked from.
  if (args.tsconfig) args.tsconfig = path.resolve(args.tsconfig);
  else {
    const candidate = path.join(args.fixture, 'tsconfig.json');
    if (existsSync(candidate)) args.tsconfig = candidate;
  }
  return args;
}

function listSourceFiles(root) {
  const out = [];
  const walk = (dir) => {
    for (const entry of readdirSync(dir, { withFileTypes: true })) {
      if (entry.isDirectory()) {
        if (!IGNORED_DIRS.has(entry.name)) walk(path.join(dir, entry.name));
      } else if (SOURCE_EXTS.has(path.extname(entry.name))) {
        out.push(path.join(dir, entry.name));
      }
    }
  };
  walk(root);
  return out;
}

const rel = (root, p) => path.relative(root, p).split(path.sep).join('/');
const edgeKey = (from, to) => `${from} -> ${to}`;

// --- blast-radius: one `graph` dump of the whole repo. Edges are stored
// depended-upon -> consumer, so flip to forward (importer -> importee).
function blastRadiusEdges(fixture) {
  const bin = path.join(REPO_ROOT, 'target', 'debug', 'blast-radius');
  const stdout = execFileSync(
    bin,
    ['--repo-root', fixture, '--format', 'json', 'graph'],
    { encoding: 'utf8', maxBuffer: 256 * 1024 * 1024 },
  );
  const data = JSON.parse(stdout);
  // Use the node `label` (already repo-relative, `/`-normalized) rather than
  // relativizing the absolute `file` ourselves: blast-radius canonicalizes the
  // repo root (resolving symlinks like macOS /tmp -> /private/tmp), so deriving
  // paths from `file` against an un-canonicalized fixture path would diverge.
  const idToLabel = new Map(data.nodes.map((n) => [n.id, n.label]));
  const edges = new Set();
  for (const e of data.edges) {
    const dependedUpon = idToLabel.get(e.from);
    const consumer = idToLabel.get(e.to);
    if (!dependedUpon || !consumer || dependedUpon === consumer) continue;
    edges.add(edgeKey(consumer, dependedUpon)); // forward: consumer imports depended-upon
  }
  return edges;
}

// --- dependency-cruiser: explicit file list (its directory globbing silently
// skips .tsx roots). Returns resolved internal edges plus the specifiers it
// could not resolve, so we can tell "blast-radius found an edge the reference
// missed because the reference couldn't resolve it" apart from a real extra.
// dependency-cruiser is invoked in chunks of files rather than all at once:
// a single call over thousands of file arguments exits non-zero with no output
// on CI (resource/arg-list limits not hit on dev machines). Resolution is
// per-file and targets are looked up on disk, so an edge from a file in one
// chunk to a file in another still resolves — the merged edge set is identical
// to a single call. Smaller chunks also localize any depcruise failure.
const DC_CHUNK = 400;

function runDepcruise(fixture, chunk, tsconfig) {
  const args = ['--yes', DC_VERSION, '--config', DC_CONFIG, '--output-type', 'json'];
  for (const f of chunk) args.push(rel(fixture, f));
  try {
    return JSON.parse(
      execFileSync('npx', args, {
        cwd: fixture,
        encoding: 'utf8',
        maxBuffer: 256 * 1024 * 1024,
        env: { ...process.env, BR_TSCONFIG: tsconfig || '' },
      }),
    );
  } catch (error) {
    const out = (error.stdout || '').toString();
    if (out.trimStart().startsWith('{')) return JSON.parse(out);
    const stderr = (error.stderr || '').toString();
    throw new Error(
      `dependency-cruiser failed on a ${chunk.length}-file chunk ` +
        `(status=${error.status} signal=${error.signal} code=${error.code}).\n` +
        `--- stderr (last 3000) ---\n${stderr.slice(-3000)}\n` +
        `--- stdout (last 1500) ---\n${out.slice(-1500)}`,
    );
  }
}

function referenceEdges(fixture, files, tsconfig) {
  const edges = new Set();
  const unresolved = []; // { from, module }
  for (let i = 0; i < files.length; i += DC_CHUNK) {
    const data = runDepcruise(fixture, files.slice(i, i + DC_CHUNK), tsconfig);
    for (const m of data.modules) {
      const from = m.source;
      for (const dep of m.dependencies || []) {
        if (dep.couldNotResolve) {
          unresolved.push({ from, module: dep.module });
          continue;
        }
        const to = dep.resolved;
        if (!to || to.startsWith('node_modules/') || to.includes('/node_modules/')) continue;
        // blast-radius scopes out non-code imports (CSS, images, .json, fonts)
        // as assets by design, so exclude them for an apples-to-apples code
        // comparison.
        if (!SOURCE_EXTS.has(path.extname(to))) continue;
        if (from === to) continue;
        edges.add(edgeKey(from, to));
      }
    }
  }
  return { edges, unresolved };
}

function main() {
  const args = parseArgs(process.argv.slice(2));
  if (!existsSync(path.join(REPO_ROOT, 'target', 'debug', 'blast-radius'))) {
    console.error('blast-radius debug binary not found; run `cargo build` first.');
    process.exit(1);
  }
  const files = listSourceFiles(args.fixture);
  const br = blastRadiusEdges(args.fixture);
  const { edges: ref, unresolved } = referenceEdges(args.fixture, files, args.tsconfig);

  const inBoth = [...br].filter((e) => ref.has(e));
  const onlyBr = [...br].filter((e) => !ref.has(e)).sort();
  const onlyRef = [...ref].filter((e) => !br.has(e)).sort();

  // Classify blast-radius-only edges: did the reference fail to resolve the
  // same (importer, *) and blast-radius succeed? Then blast-radius is superior.
  const unresolvedByFrom = new Map();
  for (const u of unresolved) {
    if (!unresolvedByFrom.has(u.from)) unresolvedByFrom.set(u.from, []);
    unresolvedByFrom.get(u.from).push(u.module);
  }
  const brSuperior = [];
  const brExtra = [];
  for (const e of onlyBr) {
    const from = e.split(' -> ')[0];
    if (unresolvedByFrom.has(from)) brSuperior.push(e);
    else brExtra.push(e);
  }

  const union = inBoth.length + onlyBr.length + onlyRef.length;
  const agreement = union === 0 ? 1 : inBoth.length / union;

  if (args.json) {
    console.log(JSON.stringify({
      fixture: rel(REPO_ROOT, args.fixture),
      files: files.length,
      agreement,
      inBoth: inBoth.length,
      onlyBlastRadius: onlyBr,
      onlyReference: onlyRef,
      blastRadiusSuperior: brSuperior,
      blastRadiusExtra: brExtra,
      referenceUnresolved: unresolved,
    }, null, 2));
  } else {
    const name = rel(REPO_ROOT, args.fixture);
    console.log(`\nAccuracy oracle: ${name}  (${files.length} files)`);
    console.log(`  agreement: ${(agreement * 100).toFixed(1)}%   in both: ${inBoth.length}`);
    console.log(`  blast-radius only: ${onlyBr.length}  (superior: ${brSuperior.length}, unexplained extra: ${brExtra.length})`);
    console.log(`  reference only:    ${onlyRef.length}  (potential blast-radius misses)`);
    if (brSuperior.length) {
      console.log('\n  ✓ blast-radius resolved edges the reference could not (workspace/alias wins):');
      for (const e of brSuperior) console.log(`      ${e}`);
    }
    if (onlyRef.length) {
      console.log('\n  ✗ edges the reference found but blast-radius missed:');
      for (const e of onlyRef) console.log(`      ${e}`);
    }
    if (brExtra.length) {
      console.log('\n  ? blast-radius edges the reference lacks, not explained by an unresolved');
      console.log('    import (usually type-only re-exports the reference drops) — review:');
      for (const e of brExtra) console.log(`      ${e}`);
    }
    console.log('');
  }

  // The hard signal is false negatives: edges the reference resolved that
  // blast-radius missed. Blast-radius "extras" are almost always it being more
  // complete (workspace resolution without node_modules, type-only re-exports),
  // so they are reported for review but do not fail the gate.
  if (args.strict && onlyRef.length > 0) process.exit(2);
}

main();
