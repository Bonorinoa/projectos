import assert from "node:assert/strict";
import { mkdtemp, rm, writeFile } from "node:fs/promises";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { pathToFileURL } from "node:url";
import {
  EdgeRef,
  GraphSelection,
  NativeGraph,
  NativeGraphModule,
  NativeGraphUnavailable,
  NodeRef,
  SourcePredicate,
  g,
  loadGraph,
  stringifyJson,
} from "../src/index.js";

const jsonBytes = (value: unknown): Uint8Array => new TextEncoder().encode(JSON.stringify(value));
const calls: Array<{ name: string; args: unknown[] }> = [];
const called = (name: string, ...args: unknown[]): void => {
  calls.push({ name, args });
};

const nativeModule: NativeGraphModule = {
  NativeGraphDirection: { Directed: "directed", Undirected: "undirected" },
  NativeTraversalDirection: { Out: "out", In: "in", Both: "both" },
  NativeDegreeKind: { In: "in", Out: "out", Total: "total" },
  NativeTraversalStrategy: { BreadthFirst: "breadth-first", DepthFirst: "depth-first" },
  NativeHubExpansionPolicy: {
    ExpandAll: () => ({ variant: "all" }),
    StopNonSeedAtOrAbove: (degree: number) => ({ variant: "stop", degree }),
  },
  NativeBetweennessMode: {
    Exact: () => ({ variant: "exact" }),
    Sampled: (sampleCount: number, seed: number) => ({ variant: "sampled", sampleCount, seed }),
    Auto: (exactThrough: number, sampleCount: number, seed: number) => ({ variant: "auto", exactThrough, sampleCount, seed }),
  },
  graph_from_query_response: () => nativeHandle,
};

const nativeHandle: Record<string, any> = {
  node_count: () => 2n,
  edge_count: () => 1n,
  is_directed: () => true,
  is_multigraph: () => false,
  graph_attributes_json: () => jsonBytes({ source: "test" }),
  contains_node: (id: string) => id === "n1",
  contains_edge: (id: string) => id === "e1",
  nodes: () => [
    { id: "n1", label: "User", attributes_json: jsonBytes({ name: "Ada" }) },
    { id: "n2", label: undefined, attributes_json: jsonBytes({}) },
  ],
  edges: () => [
    {
      id: "e1",
      graphify_key: "edge-key",
      source: "n1",
      target: "n2",
      label: "FOLLOWS",
      weight: 2.5,
      attributes_json: jsonBytes({ since: 2026 }),
    },
  ],
  node: (id: string) => (id === "n1" ? { id, label: "User", attributes_json: jsonBytes({ name: "Ada" }) } : undefined),
  edge: (id: string) =>
    id === "e1"
      ? { id, source: "n1", target: "n2", attributes_json: jsonBytes({}), graphify_key: undefined, label: undefined, weight: undefined }
      : undefined,
  neighbors: (...args: unknown[]) => (called("neighbors", ...args), ["n2"]),
  successors: (id: string) => (called("successors", id), ["n2"]),
  predecessors: (id: string) => (called("predecessors", id), []),
  out_edge_ids: (id: string) => (called("out_edge_ids", id), ["e1"]),
  in_edge_ids: (id: string) => (called("in_edge_ids", id), []),
  incident_edge_ids: (id: string) => (called("incident_edge_ids", id), ["e1"]),
  edges_between: (...args: unknown[]) => (called("edges_between", ...args), ["e1"]),
  has_edge_between: (...args: unknown[]) => (called("has_edge_between", ...args), true),
  degree: (...args: unknown[]) => (called("degree", ...args), { node_id: args[0], degree: 1 }),
  degrees: (...args: unknown[]) => (called("degrees", ...args), [{ node_id: "n1", degree: 1 }]),
  betweenness_centrality_async: async (...args: unknown[]) => (called("betweenness", ...args), [{ node_id: "n1", score: 1 }]),
  edge_betweenness_centrality_async: async (...args: unknown[]) => (called("edge_betweenness", ...args), [{ edge_id: "e1", score: 1 }]),
  simple_cycles_async: async (...args: unknown[]) => (called("cycles", ...args), { cycles: [["n1", "n2"]] }),
  traverse_async: async (...args: unknown[]) => (called("traverse", ...args), { visited: ["n1"] }),
  shortest_path_async: async (...args: unknown[]) => (called("shortest_path", ...args), { nodes: ["n1", "n2"] }),
  louvain_communities_async: async (...args: unknown[]) => (called("louvain", ...args), { communities: [["n1", "n2"]] }),
  spring_layout_async: async (...args: unknown[]) => (called("layout", ...args), [{ node_id: "n1", x: 0, y: 1 }]),
  induced_subgraph: (...args: unknown[]) => (called("induced", ...args), nativeHandle),
  to_undirected: (...args: unknown[]) => (called("undirected", ...args), nativeHandle),
  copy: (...args: unknown[]) => (called("copy", ...args), nativeHandle),
  compose: (...args: unknown[]) => (called("compose", ...args), nativeHandle),
  relabel: (...args: unknown[]) => (called("relabel", ...args), nativeHandle),
};

const selection = new GraphSelection({
  nodeTraversal: g().nWhere(SourcePredicate.hasKey("$id")),
  edgeTraversal: g().eWhere(SourcePredicate.hasKey("$id")),
  nodeProperties: ["z", "a", "z"],
  edgeProperties: ["weight", "kind", "kind"],
});
assert.equal(selection.direction, "directed");
assert.equal(selection.allowFullScan, false);
assert.deepEqual(selection.nodeProperties, ["a", "z"]);
assert.deepEqual(selection.edgeProperties, ["kind", "weight"]);
assert.match(stringifyJson(selection.toQueryRequest()), /__helix_graph_external_id/);

const limited = new GraphSelection({
  nodeTraversal: g().n(NodeRef.all()),
  edgeTraversal: g().e(EdgeRef.all()),
  direction: "undirected",
  externalIdentityProperty: "external_id",
  graphifyEdgeKeyProperty: "edge_key",
  weightProperty: "cost",
  maxNodes: 2,
  maxEdges: 3,
  allowFullScan: true,
});
const limitedJson = stringifyJson(limited.toQueryRequest());
assert.match(limitedJson, /"literal":3/);
assert.match(limitedJson, /"literal":4/);
assert.throws(() => new GraphSelection({ nodeTraversal: g().n(NodeRef.all()), edgeTraversal: g().e(EdgeRef.all()) }), /allowFullScan/);
for (const property of ["", "__helix_graph_reserved"]) {
  assert.throws(
    () =>
      new GraphSelection({ nodeTraversal: selection.nodeTraversal, edgeTraversal: selection.edgeTraversal, nodeProperties: [property] }),
    TypeError,
  );
}
assert.throws(
  () =>
    new GraphSelection({ nodeTraversal: selection.nodeTraversal, edgeTraversal: selection.edgeTraversal, direction: "sideways" as any }),
  /direction/,
);
for (const value of [0, -1, 1.5, Number.MAX_SAFE_INTEGER + 1]) {
  assert.throws(
    () => new GraphSelection({ nodeTraversal: selection.nodeTraversal, edgeTraversal: selection.edgeTraversal, maxNodes: value }),
    /maxNodes/,
  );
  assert.throws(
    () => new GraphSelection({ nodeTraversal: selection.nodeTraversal, edgeTraversal: selection.edgeTraversal, maxEdges: value }),
    /maxEdges/,
  );
}

const graph = new NativeGraph(nativeHandle, nativeModule);
assert.equal(graph.nodeCount, 2);
assert.equal(graph.edgeCount, 1);
assert.equal(graph.directed, true);
assert.equal(graph.multigraph, false);
assert.deepEqual(graph.attributes, { source: "test" });
assert.equal(graph.containsNode("n1"), true);
assert.equal(graph.containsEdge("e1"), true);
assert.deepEqual(graph.nodes()[0], { id: "n1", label: "User", attributes: { name: "Ada" } });
assert.deepEqual(graph.edges()[0], {
  id: "e1",
  graphifyKey: "edge-key",
  source: "n1",
  target: "n2",
  label: "FOLLOWS",
  weight: 2.5,
  attributes: { since: 2026 },
});
assert.equal(graph.node("missing"), undefined);
assert.equal(graph.edge("missing"), undefined);
assert.equal(graph.node("n1")?.id, "n1");
assert.equal(graph.edge("e1")?.id, "e1");
assert.deepEqual(graph.neighbors("n1"), ["n2"]);
assert.deepEqual(graph.neighbors("n1", "out"), ["n2"]);
assert.deepEqual(graph.neighbors("n1", "in"), ["n2"]);
assert.deepEqual(graph.successors("n1"), ["n2"]);
assert.deepEqual(graph.predecessors("n1"), []);
assert.deepEqual(graph.outEdgeIds("n1"), ["e1"]);
assert.deepEqual(graph.inEdgeIds("n1"), []);
assert.deepEqual(graph.incidentEdgeIds("n1"), ["e1"]);
assert.deepEqual(graph.edgesBetween("n1", "n2"), ["e1"]);
assert.deepEqual(graph.edgesBetween("n1", "n2", "out"), ["e1"]);
assert.equal(graph.hasEdgeBetween("n1", "n2", "in"), true);
assert.equal(graph.degree("n1").degree, 1);
assert.equal(graph.degree("n1", "in").degree, 1);
assert.equal(graph.degree("n1", "out").degree, 1);
assert.equal(graph.degrees()[0].degree, 1);
assert.equal(graph.degrees("in")[0].degree, 1);
assert.equal((await graph.betweennessCentrality())[0].score, 1);
assert.equal(
  (await graph.betweennessCentrality({ mode: "sampled", sampleCount: 2, seed: 7, normalized: false, endpoints: true, weighted: true }))[0]
    .score,
  1,
);
assert.equal((await graph.edgeBetweennessCentrality({ mode: "auto", exactThrough: 0, sampleCount: 3 }))[0].score, 1);
for (const options of [{ sampleCount: 0 }, { exactThrough: -1 }]) {
  await assert.rejects(graph.betweennessCentrality(options), TypeError);
}
assert.deepEqual(await graph.simpleCycles(4, 5), { cycles: [["n1", "n2"]] });
assert.deepEqual(await graph.traverse({ seeds: ["n1"], maxDepth: 2 }), { visited: ["n1"] });
assert.deepEqual(
  await graph.traverse({
    seeds: ["n1"],
    maxDepth: 3,
    strategy: "depthFirst",
    direction: "out",
    allowedLabels: ["FOLLOWS"],
    stopNonSeedAtOrAboveDegree: 10,
  }),
  { visited: ["n1"] },
);
assert.deepEqual(await graph.shortestPath("n1", "n2"), { nodes: ["n1", "n2"] });
assert.deepEqual(await graph.shortestPath("n1", "n2", { direction: "in", allowedLabels: ["FOLLOWS"], maxDepth: 3 }), {
  nodes: ["n1", "n2"],
});
assert.deepEqual(await graph.louvainCommunities(), { communities: [["n1", "n2"]] });
assert.deepEqual(await graph.louvainCommunities({ resolution: 2, threshold: 0.01, seed: 7, maxLevels: 3 }), {
  communities: [["n1", "n2"]],
});
assert.equal((await graph.springLayout())[0].node_id, "n1");
assert.equal(
  (
    await graph.springLayout({
      k: 2,
      iterations: 3,
      seed: 4,
      weighted: false,
      initialPositions: [{ nodeId: "n1", x: 1, y: 2 }],
    })
  )[0].node_id,
  "n1",
);
assert.equal(graph.inducedSubgraph(["n1"]).nodeCount, 2);
assert.equal(graph.toUndirected().nodeCount, 2);
assert.equal(graph.copy().nodeCount, 2);
assert.equal(graph.compose(graph.copy()).nodeCount, 2);
assert.equal(graph.relabel({ n1: "one" }).nodeCount, 2);
assert.equal(graph.relabel(new Map([["n2", "two"]])).nodeCount, 2);
assert.throws(() => graph.compose(new NativeGraph(nativeHandle, { ...nativeModule })), /same native binding module/);
assert.equal(
  calls.some((call) => call.name === "traverse"),
  true,
);

const temp = await mkdtemp(join(tmpdir(), "helixdb-graph-test-"));
const originalPackage = process.env.HELIXDB_UNIFFI_NODE_PACKAGE;
const originalPreferredPackage = process.env.HELIXDB_EMBEDDED_NODE_PACKAGE;
try {
  delete process.env.HELIXDB_EMBEDDED_NODE_PACKAGE;
  process.env.HELIXDB_UNIFFI_NODE_PACKAGE = pathToFileURL(join(temp, "missing.mjs")).href;
  await assert.rejects(loadGraph({ _graphResponse: async () => nativeHandle }, selection), NativeGraphUnavailable);

  const incomplete = join(temp, "incomplete.mjs");
  await writeFile(incomplete, "export const NativeGraphDirection = {}; export const NativeTraversalDirection = {};\n", "utf8");
  process.env.HELIXDB_UNIFFI_NODE_PACKAGE = pathToFileURL(incomplete).href;
  await assert.rejects(loadGraph({ _graphResponse: async () => nativeHandle }, selection), /does not export graph_from_query_response/);

  const complete = join(temp, "complete.mjs");
  await writeFile(
    complete,
    [
      "export const NativeGraphDirection = { Directed: 'directed', Undirected: 'undirected' };",
      "export const NativeTraversalDirection = { Out: 'out', In: 'in', Both: 'both' };",
      "export function graph_from_query_response(_spec, response) { return { node_count() { return response.length; } }; }",
    ].join("\n"),
    "utf8",
  );
  process.env.HELIXDB_UNIFFI_NODE_PACKAGE = pathToFileURL(complete).href;
  let receivedSpec: Record<string, unknown> | undefined;
  const fromBytes = await loadGraph(
    {
      _graphResponse: async (_request, spec) => {
        receivedSpec = spec;
        return new Uint8Array([1, 2, 3]);
      },
    },
    limited,
  );
  assert.equal(fromBytes.nodeCount, 3);
  assert.deepEqual(receivedSpec, { direction: "undirected", node_limit: 2, edge_limit: 3 });
  const fromHandle = await loadGraph({ _graphResponse: async () => nativeHandle }, selection);
  assert.equal(fromHandle.nodeCount, 2);
} finally {
  if (originalPackage === undefined) delete process.env.HELIXDB_UNIFFI_NODE_PACKAGE;
  else process.env.HELIXDB_UNIFFI_NODE_PACKAGE = originalPackage;
  if (originalPreferredPackage === undefined) delete process.env.HELIXDB_EMBEDDED_NODE_PACKAGE;
  else process.env.HELIXDB_EMBEDDED_NODE_PACKAGE = originalPreferredPackage;
  await rm(temp, { recursive: true, force: true });
}
