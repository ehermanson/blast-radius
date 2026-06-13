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

const TIERS = {
  minor: { label: 'Minor', emoji: '🟢' },
  moderate: { label: 'Moderate', emoji: '🟡' },
  risky: { label: 'Risky', emoji: '🟠' },
  high: { label: 'High', emoji: '🔴' },
};

const MAX_LISTED = 100;

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

  const changed = (result.roots || []).map((r) => r.file);
  const changedNote = changed.length
    ? `Changed: ${changed.slice(0, 10).map((f) => `\`${f}\``).join(', ')}` +
      (changed.length > 10 ? ` _(+${changed.length - 10} more)_` : '')
    : '';

  const lines = [MARKER, '## 🧨 blast-radius', ''];

  if (total === 0) {
    lines.push(`${tier.emoji} **No downstream files impacted** by the changed files.`);
    if (changedNote) lines.push('', changedNote);
    lines.push('', confidenceNote(result));
    return lines.filter((l) => l !== null).join('\n').trimEnd() + '\n';
  }

  // Combined impacted set: file nodes at depth >= 1, grouped by package.
  const impacted = (result.nodes || [])
    .filter((n) => n.kind === 'file' && (n.depth || 0) >= 1)
    .map((n) => n.label)
    .sort();
  const byPackage = new Map();
  for (const label of impacted) {
    const key = packageKey(label, result.workspaces || []);
    if (!byPackage.has(key)) byPackage.set(key, []);
    byPackage.get(key).push(label);
  }
  const packages = byPackage.size;
  const direct = summary.directly_affected_files || 0;
  const indirect = summary.transitively_affected_files || 0;

  lines.push(
    `${tier.emoji} **${tier.label}** — ${total} impacted ${plural(total, 'file')} across ` +
      `${packages} ${plural(packages, 'package')} (${direct} direct, ${indirect} indirect)`,
  );
  if (changedNote) lines.push('', changedNote);

  // Collapsible, grouped, capped so a huge radius can't blow the comment limit.
  lines.push('', `<details><summary>Impacted files (${total})</summary>`, '');
  let listed = 0;
  for (const [pkg, files] of [...byPackage].sort((a, b) => b[1].length - a[1].length)) {
    if (listed >= MAX_LISTED) break;
    lines.push(`**${pkg}** (${files.length})`, '');
    for (const file of files) {
      if (listed >= MAX_LISTED) break;
      lines.push(`- \`${file}\``);
      listed++;
    }
    lines.push('');
  }
  if (total > listed) lines.push(`_…and ${total - listed} more._`, '');
  lines.push('</details>');

  lines.push('', confidenceNote(result));
  return lines.filter((l) => l !== null).join('\n').trimEnd() + '\n';
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
  return `<sub>${note}</sub>`;
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
