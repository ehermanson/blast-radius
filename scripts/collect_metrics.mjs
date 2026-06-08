import { execFileSync } from "node:child_process";
import { existsSync, mkdirSync, writeFileSync } from "node:fs";

const repoRoot = process.cwd();
const binary = `${repoRoot}/target/debug/blast-radius`;

const cases = [
  {
    name: "monorepo_demo_button",
    args: [
      "--repo-root",
      "tests/fixtures/monorepo",
      "--format",
      "json",
      "file",
      "packages/ui/src/Button.tsx",
    ],
  },
  {
    name: "vite_app",
    args: [
      "--repo-root",
      "examples/vite-react-ts",
      "--format",
      "json",
      "file",
      "src/App.tsx",
    ],
  },
  {
    name: "chakra_button",
    args: [
      "--repo-root",
      "examples/chakra-ui",
      "--format",
      "json",
      "file",
      "packages/react/src/components/button/button.tsx",
    ],
  },
];

const metrics = {
  generated_at: new Date().toISOString(),
  cases: [],
};

for (const testCase of cases) {
  // repo-root is the value right after the "--repo-root" flag.
  const repoRootArg = testCase.args[testCase.args.indexOf("--repo-root") + 1];
  if (!existsSync(`${repoRoot}/${repoRootArg}`)) {
    console.warn(
      `skipping ${testCase.name}: ${repoRootArg} not present (run scripts/fetch-examples.sh)`,
    );
    continue;
  }
  const raw = execFileSync(binary, testCase.args, {
    cwd: repoRoot,
    encoding: "utf8",
    maxBuffer: 32 * 1024 * 1024,
  });
  const json = JSON.parse(raw);
  metrics.cases.push({
    name: testCase.name,
    total_affected_files: json.summary.total_affected_files,
    unresolved_imports: json.summary.unresolved_imports,
    ambiguous_edges: json.summary.ambiguous_edges,
    parse_failures: json.summary.parse_failures,
    warnings: json.warnings.length,
  });
}

mkdirSync(`${repoRoot}/target/quality`, { recursive: true });
writeFileSync(
  `${repoRoot}/target/quality/metrics.json`,
  `${JSON.stringify(metrics, null, 2)}\n`,
);

console.log(JSON.stringify(metrics, null, 2));
