// Unit tests for the PR-comment renderer. Pure: static JSON in, Markdown out,
// no binary or network. Run with: node --test scripts/pr-comment.test.mjs
import { test } from 'node:test';
import assert from 'node:assert/strict';

import { renderComment, MARKER } from './pr-comment.mjs';

const impactResult = {
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
  roots: [{ file: 'packages/ui/src/Button.tsx' }],
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

test('headline reports tier, totals, and package count', () => {
  const md = renderComment(impactResult);
  assert.match(md, /\*\*Moderate\*\*/);
  assert.match(md, /5 impacted files across 2 packages/);
  assert.match(md, /4 direct, 1 indirect/);
});

test('lists only downstream file nodes (depth >= 1), grouped by package', () => {
  const md = renderComment(impactResult);
  // apps/storefront has 3 impacted (App, PromoCard, LegacyButtonCard); ui has 2.
  assert.match(md, /\*\*apps\/storefront\*\* \(3\)/);
  assert.match(md, /\*\*packages\/ui\*\* \(2\)/);
  assert.match(md, /- `apps\/storefront\/src\/App\.tsx`/);
  // The changed root (depth 0) and export-kind nodes are not listed as impacted.
  assert.ok(!md.includes('- `packages/ui/src/Button.tsx`'));
  assert.ok(!md.includes('#Button'));
});

test('zero impact renders a clear no-impact message, not an empty list', () => {
  const md = renderComment({
    summary: { total_affected_files: 0, risk_tier: 'minor' },
    roots: [{ file: 'apps/storefront/src/App.tsx' }],
    nodes: [],
    workspaces: [],
  });
  assert.match(md, /No downstream files impacted/);
  assert.ok(!md.includes('<details>'));
});

test('confidence note surfaces analyzer caveats', () => {
  const md = renderComment({
    ...impactResult,
    summary: { ...impactResult.summary, unresolved_imports: 3, parse_failures: 1 },
  });
  assert.match(md, /confidence: partial — 3 unresolved imports, 1 parse failures/);
});

test('caps very large impacted lists so the comment cannot overflow', () => {
  const nodes = Array.from({ length: 250 }, (_, i) => ({
    kind: 'file',
    label: `src/file-${i}.ts`,
    depth: 1,
  }));
  const md = renderComment({
    summary: { total_affected_files: 250, directly_affected_files: 250, transitively_affected_files: 0, risk_tier: 'high' },
    roots: [{ file: 'src/hub.ts' }],
    nodes,
    workspaces: [],
  });
  assert.match(md, /…and \d+ more\./);
  assert.ok(md.length < 65000, 'comment must stay under the GitHub comment limit');
});
