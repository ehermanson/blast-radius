#!/usr/bin/env node
// Real-usage scoreboard for blast-radius.
//
// Separates human signal (stars, issues, unique visitors, repos that wired the
// Action into a workflow) from noise that inflates raw counts (security
// scanners, registry mirrors, CI, and your own release/Action testing). Early
// on, npm/clone/crate counts are almost entirely noise — watch the SIGNAL block
// and the *trend* after you announce, not the absolute download numbers.
//
//   node scripts/usage.mjs      (GitHub stats need `gh` authenticated;
//                                traffic stats need push access to the repo)

import { execFileSync } from 'node:child_process';

const REPO = 'ehermanson/blast-radius';
const NPM_PKG = 'blast-radius-cli';
const CRATE = 'blast-radius';

function gh(args) {
  try {
    return JSON.parse(execFileSync('gh', args, { encoding: 'utf8', stdio: ['ignore', 'pipe', 'ignore'] }));
  } catch {
    return null;
  }
}

async function getJson(url) {
  try {
    const res = await fetch(url, { headers: { 'User-Agent': 'blast-radius-usage' } });
    return res.ok ? await res.json() : null;
  } catch {
    return null;
  }
}

const row = (label, value) => `  ${label.padEnd(24, '.')} ${value}`;
const na = '— (unavailable)';

async function main() {
  const repo = gh(['api', `repos/${REPO}`]);
  const views = gh(['api', `repos/${REPO}/traffic/views`]);
  const clones = gh(['api', `repos/${REPO}/traffic/clones`]);
  const referrers = gh(['api', `repos/${REPO}/traffic/popular/referrers`]);

  // Public repos that reference the Action in a workflow — the strongest proxy
  // for real Action adoption.
  let adopters = null;
  try {
    const found = JSON.parse(
      execFileSync('gh', ['search', 'code', `uses: ${REPO}`, '--json', 'repository', '-L', '100'], {
        encoding: 'utf8',
        stdio: ['ignore', 'pipe', 'ignore'],
      }),
    );
    adopters = [...new Set(found.map((f) => f.repository.nameWithOwner))];
  } catch {
    adopters = null;
  }

  const npm = await getJson(`https://api.npmjs.org/downloads/range/last-month/${NPM_PKG}`);
  const crate = await getJson(`https://crates.io/api/v1/crates/${CRATE}`);

  console.log(`\nblast-radius — usage scoreboard (${REPO})\n`);

  console.log('REAL SIGNAL (humans; hard to fake)');
  console.log(row('stars', repo ? repo.stargazers_count : na));
  console.log(row('forks', repo ? repo.forks_count : na));
  console.log(row('open issues', repo ? repo.open_issues_count : na));
  console.log(row('unique visitors 14d', views ? views.uniques : na));
  console.log(
    row('repos using the Action', adopters ? `${adopters.length}${adopters.length ? ` — ${adopters.join(', ')}` : ''}` : na),
  );
  if (referrers && referrers.length) {
    const top = referrers.slice(0, 4).map((r) => `${r.referrer} (${r.uniques})`).join(', ');
    console.log(row('top referrers', top));
  } else {
    console.log(row('top referrers', referrers ? 'none' : na));
  }

  console.log('\nNOISE (inflated by bots / mirrors / CI / your own testing — watch the TREND)');
  if (npm && Array.isArray(npm.downloads)) {
    const daily = npm.downloads.map((d) => d.downloads);
    const total = daily.reduce((a, b) => a + b, 0);
    const last7 = daily.slice(-7).join(', ');
    const zeros = daily.filter((d) => d === 0).length;
    const shape = zeros > daily.length / 3 ? 'bursty/zeros = bot-like' : 'steady';
    console.log(row('npm downloads 30d', `${total}  (${shape})`));
    console.log(row('  last 7 days', `[${last7}]`));
  } else {
    console.log(row('npm downloads 30d', na));
  }
  console.log(
    row('github clones 14d', clones ? `${clones.count} (${clones.uniques} unique)` : na),
  );
  if (clones && views) {
    console.log(
      row('  clones vs visitors', `${clones.uniques} cloners / ${views.uniques} visitors → mostly automation if lopsided`),
    );
  }
  console.log(row('crates downloads', crate?.crate ? crate.crate.downloads : na));

  console.log('\nReal adoption = the SIGNAL block growing after you tell people it exists.');
  console.log('One inbound issue or a repo wiring in the Action outweighs any download count.\n');
}

main();
