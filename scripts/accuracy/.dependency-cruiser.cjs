// dependency-cruiser config for the accuracy oracle. No lint rules — we only
// use dependency-cruiser as a reference graph extractor and diff its edges
// against blast-radius. The per-fixture tsconfig is passed via BR_TSCONFIG so
// path aliases resolve the same way the fixture's own tooling sees them.
const tsConfigFileName = process.env.BR_TSCONFIG || undefined;

/** @type {import('dependency-cruiser').IConfiguration} */
module.exports = {
  forbidden: [],
  options: {
    // node_modules is not part of a repo's own blast radius; blast-radius never
    // indexes it, so the reference must not either.
    doNotFollow: { path: 'node_modules' },
    // Resolve the same extension family blast-radius does, TS source first.
    enhancedResolveOptions: {
      extensions: ['.ts', '.tsx', '.js', '.jsx', '.mjs', '.cjs', '.mts', '.cts'],
      // Honor package.json "exports"/"imports" and the conditions a bundler
      // would, matching blast-radius's resolver.
      exportsFields: ['exports'],
      conditionNames: ['import', 'require', 'node', 'default', 'types'],
    },
    // Include type-only imports: blast-radius counts them as edges too.
    tsPreCompilationDeps: true,
    ...(tsConfigFileName ? { tsConfig: { fileName: tsConfigFileName } } : {}),
  },
};
