# Reading the output

The default `tree` output leads with a **risk verdict** — `minor`, `moderate`,
`risky`, or `high` — plus a meter and the counts behind it. For larger blast
radii a **hotspots** chart follows, showing the directories with the most
impacted files, so you can see where the change lands before reading any paths.

The impacted files are then listed grouped by package and directory. Files
marked `◎ endpoint` are leaves nothing else depends on (apps, routes, pages) —
a signal the change can reach something user-facing. Past 200 impacted files
the per-file lists collapse to directory rollups; pass `-v` to list every file.

Pass `--verbose` (`-v`) to see the full root → cascade tree of exactly how the
impact propagates.

## Confidence

The result ends with a confidence read so you know how much to trust it:

- **high** — every import edge on the impacted paths resolved cleanly.
- **partial — N ambiguous edges on these paths** — some edges the result was
  traced through couldn't be pinned to a single target (e.g. a symbol re-exported
  by several barrels), so a few listed files may be over-attributed.

Two repo-wide caveats can be appended because their targets are unknown, so they
might hide *additional* consumers not listed:

- **N unresolved imports** — internal-looking imports that didn't resolve to a
  file (often generated/virtual modules or build output). Quiet these with
  `.blast-radius.json` (see [configuration](configuration.md)).
- **N parse failures** — files that couldn't be parsed and were skipped.
