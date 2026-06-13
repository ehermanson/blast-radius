// Unit tests for the PR-comment renderer. Pure: static JSON in, Markdown out,
// no binary or network. Run with: node --test scripts/pr-comment.test.mjs
import { test } from 'node:test';
import assert from 'node:assert/strict';

import { renderComment, MARKER } from './pr-comment.mjs';

const impactResult = {
  repo_root: '/repo',
  target: { kind: 'files', files: ['/repo/packages/ui/src/Button.tsx'] },
  summary: {
    directly_affected_files: 4,
    transitively_affected_files: 1,
    total_affected_files: 5,
    unresolved_imports: 0,
    ambiguous_edges: 0,
    parse_failures: 0,
    skipped_inputs: 0,
    risk_tier: 'moderate',
  },
  workspaces: [
    { name: '@acme/ui', root: 'packages/ui' },
    { name: '@acme/storefront', root: 'apps/storefront' },
  ],
  roots: [],
  nodes: [
    { kind: 'file', label: 'packages/ui/src/Button.tsx', depth: 0 },
    { kind: 'file', label: 'packages/ui/src/Card.tsx', depth: 1 },
    { kind: 'file', label: 'packages/ui/src/index.ts', depth: 1 },
    { kind: 'file', label: 'apps/storefront/src/App.tsx', depth: 2 },
    { kind: 'file', label: 'apps/storefront/src/PromoCard.tsx', depth: 1 },
    { kind: 'file', label: 'apps/storefront/src/LegacyButtonCard.jsx', depth: 1 },
    { kind: 'export', label: 'packages/ui/src/Button.tsx#Button', depth: 0 },
  ],
};

test('includes the sticky marker so the action can update in place', () => {
  assert.ok(renderComment(impactResult).startsWith(MARKER));
});

test('shows the changed file(s) that were touched, repo-relative', () => {
  assert.match(renderComment(impactResult), /\*\*Changed:\*\* `packages\/ui\/src\/Button\.tsx`/);
});

test('headline reports tier, totals, and package count', () => {
  const md = renderComment(impactResult);
  assert.match(md, /\*\*Moderate\*\*/);
  assert.match(md, /5 impacted files across 2 packages/);
  assert.match(md, /4 direct, 1 indirect/);
});

test('lists impacted files grouped by directory, as basenames, excluding the changed file', () => {
  const md = renderComment(impactResult);
  // apps/storefront/src has 3 impacted (App, PromoCard, LegacyButtonCard).
  assert.match(md, /\*\*`apps\/storefront\/src`\*\* \(3\)/);
  assert.match(md, /- App\.tsx/);
  // The list uses basenames, not full paths.
  assert.ok(!md.includes('- apps/storefront/src/App.tsx'));
  // The changed root and export-kind nodes are not listed as impacted.
  assert.ok(!md.includes('- Button.tsx'));
  assert.ok(!md.includes('#Button'));
});

test('zero impact renders a clear message and still shows what changed', () => {
  const md = renderComment({
    repo_root: '/repo',
    target: { kind: 'files', files: ['/repo/apps/storefront/src/App.tsx'] },
    summary: { total_affected_files: 0, risk_tier: 'minor' },
    roots: [],
    nodes: [],
    workspaces: [],
  });
  assert.match(md, /No downstream files impacted/);
  assert.match(md, /\*\*Changed:\*\* `apps\/storefront\/src\/App\.tsx`/);
  assert.ok(!md.includes('<details>'));
});

test('unresolved imports / parse failures are caveats, not a "partial" verdict', () => {
  // Matches the CLI: repo-wide blind spots stay "high" with an appended caveat;
  // only on-path ambiguity downgrades the verdict.
  const md = renderComment({
    ...impactResult,
    summary: { ...impactResult.summary, unresolved_imports: 187, parse_failures: 1 },
  });
  assert.match(md, /confidence: high/);
  assert.match(md, /187 unresolved imports repo-wide may hide consumers/);
  assert.match(md, /1 parse failures may hide consumers/);
  assert.ok(!md.includes('partial'));
});

test('ambiguous edges on the impacted paths downgrade the verdict to partial', () => {
  const md = renderComment({
    ...impactResult,
    edges: [
      { from: 'a', to: 'b', kind: 'reexports_star', is_ambiguous: true },
      { from: 'b', to: 'c', kind: 'imports_named', is_ambiguous: false },
    ],
  });
  assert.match(md, /confidence: partial — 1 ambiguous edge on these paths/);
});

test('large radii get a "where it lands" summary and a capped list', () => {
  const nodes = Array.from({ length: 250 }, (_, i) => ({
    kind: 'file',
    label: `src/dir-${i % 10}/file-${i}.ts`,
    depth: 1,
  }));
  const md = renderComment({
    repo_root: '/repo',
    target: { kind: 'files', files: ['/repo/src/hub.ts'] },
    summary: {
      total_affected_files: 250,
      directly_affected_files: 250,
      transitively_affected_files: 0,
      risk_tier: 'high',
    },
    roots: [],
    nodes,
    workspaces: [],
  });
  assert.match(md, /\*\*Where it lands\*\*/);
  assert.match(md, /…and 4 more directories/);
  assert.match(md, /…and \d+ more\./);
  assert.ok(md.length < 65000, 'comment must stay under the GitHub comment limit');
});
