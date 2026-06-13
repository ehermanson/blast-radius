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

test('lists impacted files as a flat list of repo-relative paths, excluding the changed file', () => {
  const md = renderComment(impactResult);
  assert.match(md, /- `apps\/storefront\/src\/App\.tsx`/);
  // No directory-grouping headers or "where it lands" summary.
  assert.ok(!md.includes('Where it lands'));
  assert.ok(!md.includes('**`apps/storefront/src`**'));
  // The changed root and export-kind nodes are not listed as impacted.
  assert.ok(!md.includes('- `packages/ui/src/Button.tsx`'));
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

test('footer is a small footnote linking back to the project, no divider', () => {
  const md = renderComment(impactResult);
  assert.match(md, /<sub>confidence: high · <a href="https:\/\/github\.com\/ehermanson\/blast-radius">blast-radius<\/a><\/sub>/);
  assert.ok(!md.includes('---'));
  assert.ok(!md.includes('─'));
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

test('multiple changed files get a per-file impact breakdown', () => {
  const md = renderComment({
    repo_root: '/repo',
    target: { kind: 'files', files: ['/repo/a.ts', '/repo/b.ts'] },
    summary: {
      total_affected_files: 2,
      directly_affected_files: 2,
      transitively_affected_files: 0,
      risk_tier: 'moderate',
    },
    workspaces: [],
    nodes: [
      { kind: 'file', label: 'src/x.ts', depth: 1 },
      { kind: 'file', label: 'src/y.ts', depth: 1 },
    ],
    roots: [
      {
        file: 'a.ts',
        affected: 2,
        direct: 2,
        indirect: 0,
        files: [
          { path: 'src/x.ts', depth: 1, endpoint: false },
          { path: 'src/y.ts', depth: 1, endpoint: true },
        ],
      },
      { file: 'b.ts', affected: 0, direct: 0, indirect: 0, files: [] },
    ],
  });
  assert.match(md, /\*\*What each changed file reaches\*\*/);
  // Each changed file gets attributed impact, with the path as <code> (renders
  // inside <summary>, where markdown backticks would not).
  assert.match(md, /<code>a\.ts<\/code> — 2 impacted files \(2 direct, 0 indirect\)/);
  assert.match(md, /- `src\/x\.ts`/);
  // ...including "this change reaches nothing".
  assert.match(md, /<code>b\.ts<\/code> — no downstream impact/);
});

test('a huge radius is capped so the comment stays under the size limit', () => {
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
  assert.match(md, /…and 150 more/);
  assert.ok(md.length < 65000, 'comment must stay under the GitHub comment limit');
});
