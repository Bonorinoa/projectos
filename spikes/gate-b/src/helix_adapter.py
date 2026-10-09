from __future__ import annotations

from pathlib import Path
from typing import Iterable

from helixdb import (
    BatchCondition,
    Client,
    Disk,
    EdgeRef,
    SourcePredicate,
    g,
    read_batch,
    write_batch,
)

from common import Edge, Node


class HelixAdapter:
    engine = "helix"

    def __init__(self, path: Path) -> None:
        self.path = path
        self.path.mkdir(parents=True, exist_ok=True)
        self.client = Client.embedded(Disk(str(path), "projectos"))

    @staticmethod
    def _node_props(node: Node) -> dict[str, object]:
        return {
            "stable_id": node.stable_id,
            "kind": node.kind,
            "name": node.name,
            "body": node.body,
            "schema_version": node.schema_version,
        }

    @staticmethod
    def _edge_props(edge: Edge) -> dict[str, object]:
        return {
            "stable_id": edge.stable_id,
            "kind": edge.kind,
            "position": edge.position,
        }

    def transactional_seed(self, nodes: Iterable[Node], edges: Iterable[Edge]) -> None:
        batch = write_batch()
        vars_by_id: dict[str, str] = {}
        for idx, node in enumerate(nodes):
            name = f"node_{idx}"
            vars_by_id[node.stable_id] = name
            batch = batch.var_as(name, g().add_n("Entity", self._node_props(node)))
        for idx, edge in enumerate(edges):
            batch = batch.var_as(
                f"edge_{idx}",
                g().n(vars_by_id[edge.source]).add_e(
                    edge.kind, vars_by_id[edge.target], self._edge_props(edge)
                ),
            )
        self.client.query(batch.returning([]).to_query_request())

    def upsert_node(self, node: Node) -> None:
        update = g().n("existing")
        for key, value in self._node_props(node).items():
            if key != "stable_id":
                update = update.set_property(key, value)
        batch = (
            write_batch()
            .var_as("existing", g().n_with_label("Entity").has("stable_id", node.stable_id))
            .var_as_if(
                "created",
                BatchCondition.var_empty("existing"),
                g().add_n("Entity", self._node_props(node)),
            )
            .var_as_if("updated", BatchCondition.var_not_empty("existing"), update)
            .returning([])
        )
        self.client.query(batch.to_query_request())

    def upsert_edge(self, edge: Edge) -> None:
        update = g().e(EdgeRef.var("existing"))
        for key, value in self._edge_props(edge).items():
            if key != "stable_id":
                update = update.set_property(key, value)
        batch = (
            write_batch()
            .var_as("source", g().n_with_label("Entity").has("stable_id", edge.source))
            .var_as("target", g().n_with_label("Entity").has("stable_id", edge.target))
            .var_as(
                "existing",
                g().n("source").out_e(edge.kind).edge_has("stable_id", edge.stable_id),
            )
            .var_as_if(
                "created",
                BatchCondition.var_empty("existing"),
                g().n("source").add_e(edge.kind, "target", self._edge_props(edge)),
            )
            .var_as_if("updated", BatchCondition.var_not_empty("existing"), update)
            .returning([])
        )
        self.client.query(batch.to_query_request())

    def delete_edge(self, stable_id: str) -> None:
        batch = (
            write_batch()
            .var_as("existing", g().e_where(SourcePredicate.eq("stable_id", stable_id)))
            .var_as("source", g().e(EdgeRef.var("existing")).in_n())
            .var_as_if(
                "deleted",
                BatchCondition.var_not_empty("existing"),
                g().n("source").drop_edge_by_id(EdgeRef.var("existing")),
            )
            .returning([])
        )
        self.client.query(batch.to_query_request())

    def delete_node(self, stable_id: str) -> None:
        batch = (
            write_batch()
            .var_as("existing", g().n_with_label("Entity").has("stable_id", stable_id))
            .var_as_if(
                "deleted",
                BatchCondition.var_not_empty("existing"),
                g().n("existing").drop(),
            )
            .returning([])
        )
        self.client.query(batch.to_query_request())

    def traverse(self, stable_id: str, *, direction: str, kind: str, hops: int) -> list[str]:
        if direction not in {"out", "in"} or hops not in {1, 2}:
            raise ValueError("direction must be out/in and hops must be 1/2")
        traversal = g().n_with_label("Entity").has("stable_id", stable_id)
        for _ in range(hops):
            if direction == "out":
                traversal = traversal.out(kind)
            else:
                traversal = traversal.in_(kind)
        request = (
            read_batch()
            .var_as("matches", traversal.value_map(["stable_id"]))
            .returning(["matches"])
            .to_query_request()
        )
        rows = self.client.query(request).get("matches", [])
        return sorted({str(row["stable_id"]) for row in rows})

    def export(self) -> tuple[list[Node], list[Edge]]:
        node_request = (
            read_batch()
            .var_as("nodes", g().n_with_label("Entity").value_map())
            .returning(["nodes"])
            .to_query_request()
        )
        edge_request = (
            read_batch()
            .var_as(
                "edges",
                g().e_where(SourcePredicate.has_key("stable_id")).edge_properties(),
            )
            .returning(["edges"])
            .to_query_request()
        )
        raw_nodes = self.client.query(node_request).get("nodes", [])
        raw_edges = self.client.query(edge_request).get("edges", [])
        id_to_stable = {int(row["$id"]): str(row["stable_id"]) for row in raw_nodes}
        nodes = [
            Node(
                stable_id=str(row["stable_id"]),
                kind=str(row["kind"]),
                name=str(row["name"]),
                body=str(row["body"]),
                schema_version=int(row["schema_version"]),
            )
            for row in raw_nodes
        ]
        edges = [
            Edge(
                stable_id=str(row["stable_id"]),
                kind=str(row["kind"]),
                source=id_to_stable[int(row["$from"])],
                target=id_to_stable[int(row["$to"])],
                position=int(row["position"]),
            )
            for row in raw_edges
        ]
        return sorted(nodes, key=lambda n: n.stable_id), sorted(edges, key=lambda e: e.stable_id)

    def checkpoint(self) -> None:
        # Embedded query completion is the only public durability boundary exposed
        # by the Python runtime; close/reopen is exercised separately.
        return None

    def close(self) -> None:
        self.client.close()
