#!/usr/bin/env node
// Render a blast-radius `files --format json` result into a Markdown PR comment.
//
// This is deliberately decoupled from GitHub: it reads the analyzer's JSON on
// stdin and writes Markdown to stdout, so it can be unit-tested locally with no
// network or Actions runner (see pr-comment.test.mjs). The action.yml wrapper
// only handles the GitHub plumbing (changed files in, comment out).
//
//   blast-radius --repo-root . --format json files - < changed.txt \
//     | node scripts/pr-comment.mjs

// Stable marker so the action can find and update its own comment in place.
export const MARKER = '<!-- blast-radius -->';

const REPO_URL = 'https://github.com/ehermanson/blast-radius';
const CONFIDENCE_URL = `${REPO_URL}#confidence`;

const TIERS = {
  minor: { label: 'Minor', emoji: '🟢' },
  moderate: { label: 'Moderate', emoji: '🟡' },
  risky: { label: 'Risky', emoji: '🟠' },
  high: { label: 'High', emoji: '🔴' },
};

const MAX_LISTED = 100;
const MAX_ROOTS = 20;
// Global budget on enumerated file paths across ALL per-file sections, so a
// many-file PR can't produce a comment past GitHub's 65,536-char limit. Each
// changed file gets a fair share (at least MIN_PER_ROOT) so no single huge
// change starves the rest. ~250 paths keeps the body well under the limit.
const MAX_COMMENT_LIST = 250;
const MIN_PER_ROOT = 8;

// Hotspot chart sizing, mirrored from the terminal report (src/report/tree.rs)
// so the two surfaces tell the same story.
const HOTSPOT_ROWS = 6;
const HOTSPOT_MIN_FILES = 8;
const BAR_CELLS = 14;
const LABEL_MAX = 34;

// Mirror of the Rust `package_key`: the longest matching workspace root, else
// the top-level directory.
function packageKey(rel, workspaces) {
  const root = workspaces
    .map((w) => w.root)
    .filter((r) => r === '' || rel === r || rel.startsWith(`${r}/`))
    .sort((a, b) => b.length - a.length)[0];
  if (root !== undefined) return root === '' ? '.' : root;
  const slash = rel.indexOf('/');
  return slash === -1 ? '.' : rel.slice(0, slash);
}

export function renderComment(result) {
  // Never throw on malformed input: a non-object (null, array, parse leftovers)
  // degrades to an empty result rather than crashing the action's comment step.
  if (!result || typeof result !== 'object' || Array.isArray(result)) return fallbackComment();
  const summary = result.summary || {};
  const total = summary.total_affected_files || 0;
  const tier = TIERS[summary.risk_tier] || { label: summary.risk_tier || 'unknown', emoji: '•' };
  const changed = changedFiles(result);

  const lines = [MARKER, '## 🧨 blast-radius', ''];

  if (total === 0) {
    lines.push(`${tier.emoji} **No downstream files impacted** by the changed files.`);
    lines.push('', changedSection(changed));
    lines.push('', confidenceNote(result));
    return finalize(lines);
  }

  const impacted = (result.nodes || [])
    .filter((n) => n.kind === 'file' && (n.depth || 0) >= 1)
    .map((n) => n.label);
  const packages = new Set(impacted.map((l) => packageKey(l, result.workspaces || []))).size;
  const direct = summary.directly_affected_files || 0;
  const indirect = summary.transitively_affected_files || 0;

  lines.push(
    `${tier.emoji} **${tier.label}** — ${total} impacted ${plural(total, 'file')} across ` +
      `${packages} ${plural(packages, 'package')} (${direct} direct, ${indirect} indirect)`,
  );

  // "Where it lands": a two-tone hotspot chart over ALL impacted files (single
  // or multi), the overview to the per-file checklists below. Mirrors the
  // terminal HOTSPOTS chart, with each bar split direct (█) vs transitive (░).
  const entries = (result.nodes || [])
    .filter((n) => n.kind === 'file' && (n.depth || 0) >= 1)
    .map((n) => ({ path: n.label, depth: n.depth || 0 }));
  if (total >= HOTSPOT_MIN_FILES) {
    const chart = hotspotSection(entries);
    if (chart.length) lines.push('', ...chart);
  }

  const roots = (result.roots || []).slice().sort((a, b) => b.affected - a.affected);
  if (roots.length > 1) {
    // Multiple changed files: attribute impact to each one (the combined total
    // in the headline double-counts files reachable from more than one input).
    lines.push('', '**What each changed file reaches**');
    const shownRoots = roots.slice(0, MAX_ROOTS);
    let listBudget = MAX_COMMENT_LIST;
    shownRoots.forEach((root, idx) => {
      if (!root.affected) {
        lines.push(
          '',
          `<details><summary><code>${root.file}</code> — no downstream impact</summary>`,
          '',
          '_Nothing depends on this file._',
          '</details>',
        );
        return;
      }
      // Lead with the direct consumers (the actionable review checklist —
      // files that actually import the changed file). Transitive reach is a
      // magnitude, not a checklist: a hub like a route tree pulls the whole app
      // into the radius, so enumerating it per-root is noise. Keep it as a count.
      const directs = (root.files || []).filter((f) => f.depth === 1).map((f) => f.path);
      const summaryLine =
        `<code>${root.file}</code> — ${root.direct} direct` +
        (root.indirect ? ` · ${root.indirect} indirect` : '') +
        ` (${root.affected} total)`;
      lines.push('', `<details><summary>${summaryLine}</summary>`, '');
      // Fair share of the remaining global budget, so one huge change can't eat
      // the whole comment and starve later files (and we stay under the limit).
      const rootsLeft = shownRoots.length - idx;
      const allowance = Math.min(MAX_LISTED, Math.max(MIN_PER_ROOT, Math.floor(listBudget / rootsLeft)));
      lines.push(...impactList(directs, allowance));
      listBudget -= Math.min(directs.length, allowance);
      if (root.indirect) {
        lines.push(
          '',
          `<sub>+ ${root.indirect} ${plural(root.indirect, 'file')} reached indirectly (transitive)</sub>`,
        );
      }
      lines.push('</details>');
    });
    if (roots.length > MAX_ROOTS) {
      lines.push('', `_…and ${roots.length - MAX_ROOTS} more changed files._`);
    }
  } else {
    // Single changed file: lead with direct consumers, then transitive reach.
    lines.push('', changedSection(changed));
    const entries = (result.nodes || [])
      .filter((n) => n.kind === 'file' && (n.depth || 0) >= 1)
      .map((n) => ({ path: n.label, depth: n.depth || 0 }));
    lines.push('', ...impactSection(entries, direct, indirect));
  }

  lines.push('', confidenceNote(result));
  return finalize(lines);
}

const finalize = (lines) => lines.filter((l) => l !== null).join('\n').trimEnd() + '\n';

// A graceful, sticky comment for when there's no usable analysis to render
// (the analyzer emitted nothing or invalid JSON). Keeps the MARKER so it
// updates the existing comment in place instead of leaving a stale report.
export function fallbackComment() {
  return finalize([
    MARKER,
    '## 🧨 blast-radius',
    '',
    "⚠️ Couldn't read the blast-radius analysis, so there's nothing to report for this change.",
    '',
    `<sub><a href="${REPO_URL}">blast-radius</a></sub>`,
  ]);
}

// A flat, alphabetical list of impacted files (repo-relative paths), capped so a
// huge radius can't blow GitHub's comment size limit.
function impactList(labels, max = MAX_LISTED) {
  const sorted = [...labels].sort();
  const lines = sorted.slice(0, max).map((f) => `- \`${f}\``);
  if (sorted.length > max) lines.push(`- _…and ${sorted.length - max} more_`);
  return lines;
}

// Direct consumers up front (the review checklist), transitive reach demoted to
// a collapsed, depth-ordered drill-down. `entries` is [{path, depth}] with the
// changed file (depth 0) already excluded; counts come from the summary so the
// section agrees with the headline.
function impactSection(entries, directCount, indirectCount) {
  const directs = entries.filter((e) => e.depth === 1).map((e) => e.path);
  const indirects = entries.filter((e) => e.depth > 1);
  const out = [];

  if (directs.length) {
    out.push(`<details><summary>${directCount} direct ${plural(directCount, 'consumer')}</summary>`, '');
    out.push(...impactList(directs));
    out.push('</details>');
  } else {
    out.push('_No direct consumers — every impacted file is reached transitively._');
  }

  if (indirects.length) {
    out.push(
      '',
      `<details><summary>${indirectCount} more reached indirectly (transitive)</summary>`,
      '',
    );
    out.push(...impactListByDepth(indirects));
    out.push('</details>');
  }
  return out;
}

// A two-tone "where it lands" chart: impacted files bucketed by their parent
// directory, busiest first, each bar split into direct (█) and transitive (░)
// reach. This is the magnitude+spread overview a tree can't give without
// exploding on fan-in. Rendered in a code block so the monospace bars align in
// both the terminal and a GitHub comment. Returns [] when there's nothing worth
// charting (fewer than two areas). `entries` is [{path, depth}], depth >= 1.
function hotspotSection(entries) {
  const areas = new Map();
  for (const e of entries) {
    const dir = splitDir(e.path);
    const a = areas.get(dir) || { direct: 0, indirect: 0 };
    if (e.depth === 1) a.direct += 1;
    else a.indirect += 1;
    areas.set(dir, a);
  }
  if (areas.size < 2) return [];

  const rows = [...areas.entries()]
    .map(([dir, c]) => ({ dir, direct: c.direct, indirect: c.indirect, total: c.direct + c.indirect }))
    .sort((a, b) => b.total - a.total || a.dir.localeCompare(b.dir));
  const shown = rows.slice(0, HOTSPOT_ROWS);
  const max = Math.max(...shown.map((r) => r.total), 1);
  const labels = shown.map((r) => clipLeft(dirLabel(r.dir), LABEL_MAX));
  const width = Math.max(...labels.map((l) => l.length));

  const out = ['**Impact by area** — █ direct · ░ transitive', '', '```'];
  shown.forEach((r, i) => {
    const filled = Math.min(BAR_CELLS, Math.ceil((r.total * BAR_CELLS) / max));
    const filledDirect = r.total ? Math.min(filled, Math.round((r.direct / r.total) * filled)) : 0;
    const bar =
      '█'.repeat(filledDirect) +
      '░'.repeat(filled - filledDirect) +
      ' '.repeat(BAR_CELLS - filled);
    const counts = `${String(r.direct).padStart(3)} │${String(r.indirect).padStart(3)}`;
    out.push(`${labels[i].padEnd(width)}  ${bar}  ${counts}`);
  });
  const more = rows.length - shown.length;
  if (more > 0) out.push(`+${more} more ${more === 1 ? 'directory' : 'directories'}`);
  out.push('```');
  return out;
}

const splitDir = (path) => {
  const i = Math.max(path.lastIndexOf('/'), path.lastIndexOf('\\'));
  return i === -1 ? '' : path.slice(0, i);
};
const dirLabel = (dir) => (dir === '' ? './' : `${dir}/`);
// Truncate from the left, keeping the most specific (rightmost) path segments.
const clipLeft = (text, max) => (text.length <= max ? text : `…${text.slice(text.length - (max - 1))}`);

// Like `impactList`, but ordered nearest-first (by hop distance, then path) so
// the closest dependents read at the top — a flat stand-in for a drill-down.
function impactListByDepth(entries) {
  const sorted = [...entries].sort((a, b) => a.depth - b.depth || a.path.localeCompare(b.path));
  const lines = sorted.slice(0, MAX_LISTED).map((e) => `- \`${e.path}\``);
  if (sorted.length > MAX_LISTED) lines.push(`- _…and ${sorted.length - MAX_LISTED} more_`);
  return lines;
}

// The files the PR actually changed (the analysis inputs), repo-relative.
function changedFiles(result) {
  const root = (result.repo_root || '').split('\\').join('/');
  const target = result.target || {};
  const raw = target.kind === 'file' ? [target.file] : target.files || [];
  return raw.filter(Boolean).map((f) => {
    const s = f.split('\\').join('/');
    return root && s.startsWith(`${root}/`) ? s.slice(root.length + 1) : s;
  });
}

function changedSection(changed) {
  if (changed.length === 0) return null;
  if (changed.length === 1) return `**Changed:** \`${changed[0]}\``;
  const shown = changed.slice(0, 15).map((f) => `- \`${f}\``);
  if (changed.length > 15) shown.push(`- _…and ${changed.length - 15} more_`);
  return [`**Changed (${changed.length}):**`, '', ...shown].join('\n');
}

// Mirrors the CLI footer: the high/partial verdict is driven ONLY by ambiguity
// on the impacted paths (the traced edges) — not by repo-wide blind spots.
// Unresolved imports and parse failures are appended as separate "may hide
// consumers" caveats, exactly as the tool reports them, so the comment never
// disagrees with `blast-radius`'s own confidence line.
function confidenceNote(result) {
  const summary = result.summary || {};
  const total = summary.total_affected_files || 0;
  const onPathAmbiguous = (result.edges || []).filter((e) => e.is_ambiguous).length;

  // "confidence" links to its explanation, so the jargon (ambiguous edges, "may
  // hide consumers") is one click from a definition without bloating the comment.
  const label = `<a href="${CONFIDENCE_URL}">confidence</a>`;
  let note =
    total === 0 || onPathAmbiguous === 0
      ? `${label}: high`
      : `${label}: partial — ${onPathAmbiguous} ambiguous ${plural(onPathAmbiguous, 'edge')} on these paths`;

  const caveats = [];
  // Coverage gap: changed files the analyzer couldn't include (missing on disk
  // or not a recognized source file). Surface it so the report never reads as
  // if it covered every changed file when it didn't.
  if (summary.skipped_inputs) {
    caveats.push(
      `${summary.skipped_inputs} changed ${plural(summary.skipped_inputs, 'file')} skipped (not analyzed)`,
    );
  }
  if (total > 0 && summary.unresolved_imports) {
    caveats.push(`${summary.unresolved_imports} unresolved imports repo-wide may hide consumers`);
  }
  if (summary.parse_failures) {
    caveats.push(`${summary.parse_failures} parse failures may hide consumers`);
  }
  if (caveats.length) note += ` · ${caveats.join(' · ')}`;
  // Small, muted footnote — no rule. The <sub> sizing is enough separation.
  return `<sub>${note} · <a href="${REPO_URL}">blast-radius</a></sub>`;
}

const plural = (n, word) => (n === 1 ? word : `${word}s`);

// Run as a script: read stdin, write Markdown to stdout.
if (import.meta.url === `file://${process.argv[1]}`) {
  let input = '';
  process.stdin.setEncoding('utf8');
  process.stdin.on('data', (chunk) => (input += chunk));
  process.stdin.on('end', () => {
    let result;
    try {
      result = JSON.parse(input);
    } catch {
      // Analyzer crashed or emitted partial/empty output: post a clear note
      // instead of a stack trace that fails the whole check with no comment.
      process.stdout.write(fallbackComment());
      return;
    }
    process.stdout.write(renderComment(result));
  });
}
