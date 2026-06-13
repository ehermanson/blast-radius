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

const TIERS = {
  minor: { label: 'Minor', emoji: '🟢' },
  moderate: { label: 'Moderate', emoji: '🟡' },
  risky: { label: 'Risky', emoji: '🟠' },
  high: { label: 'High', emoji: '🔴' },
};

const MAX_LISTED = 100;
const MAX_ROOTS = 20;

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

  const roots = (result.roots || []).slice().sort((a, b) => b.affected - a.affected);
  if (roots.length > 1) {
    // Multiple changed files: attribute impact to each one (the combined total
    // in the headline double-counts files reachable from more than one input).
    lines.push('', '**What each changed file reaches**');
    for (const root of roots.slice(0, MAX_ROOTS)) {
      if (!root.affected) {
        lines.push(
          '',
          `<details><summary><code>${root.file}</code> — no downstream impact</summary>`,
          '',
          '_Nothing depends on this file._',
          '</details>',
        );
        continue;
      }
      const summaryLine =
        `<code>${root.file}</code> — ${root.affected} impacted ${plural(root.affected, 'file')} ` +
        `(${root.direct} direct, ${root.indirect} indirect)`;
      lines.push('', `<details><summary>${summaryLine}</summary>`, '');
      lines.push(...impactList((root.files || []).map((f) => f.path)));
      lines.push('</details>');
    }
    if (roots.length > MAX_ROOTS) {
      lines.push('', `_…and ${roots.length - MAX_ROOTS} more changed files._`);
    }
  } else {
    // Single changed file: one flat list.
    lines.push('', changedSection(changed));
    lines.push('', `<details><summary>All ${total} impacted files</summary>`, '');
    lines.push(...impactList(impacted));
    lines.push('</details>');
  }

  lines.push('', confidenceNote(result));
  return finalize(lines);
}

const finalize = (lines) => lines.filter((l) => l !== null).join('\n').trimEnd() + '\n';

// A flat, alphabetical list of impacted files (repo-relative paths), capped so a
// huge radius can't blow GitHub's comment size limit.
function impactList(labels) {
  const sorted = [...labels].sort();
  const lines = sorted.slice(0, MAX_LISTED).map((f) => `- \`${f}\``);
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

  let note =
    total === 0 || onPathAmbiguous === 0
      ? 'confidence: high'
      : `confidence: partial — ${onPathAmbiguous} ambiguous ${plural(onPathAmbiguous, 'edge')} on these paths`;

  const caveats = [];
  if (total > 0 && summary.unresolved_imports) {
    caveats.push(`${summary.unresolved_imports} unresolved imports repo-wide may hide consumers`);
  }
  if (summary.parse_failures) {
    caveats.push(`${summary.parse_failures} parse failures may hide consumers`);
  }
  if (caveats.length) note += ` · ${caveats.join(' · ')}`;
  // A thin, muted rule + footer. GitHub strips CSS, so a markdown `---` would be
  // a heavy `<hr>`; a light box-drawing line kept inside <sub> reads as a subtle
  // footnote separator instead.
  const rule = '─'.repeat(36);
  return `<sub>${rule}<br>${note} · <a href="${REPO_URL}">blast-radius</a></sub>`;
}

const plural = (n, word) => (n === 1 ? word : `${word}s`);

// Run as a script: read stdin, write Markdown to stdout.
if (import.meta.url === `file://${process.argv[1]}`) {
  let input = '';
  process.stdin.setEncoding('utf8');
  process.stdin.on('data', (chunk) => (input += chunk));
  process.stdin.on('end', () => {
    process.stdout.write(renderComment(JSON.parse(input)));
  });
}
