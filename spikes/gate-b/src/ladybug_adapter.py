from __future__ import annotations

from pathlib import Path
from typing import Iterable

import ladybug

from common import Edge, Node


class LadybugAdapter:
    engine = "ladybug"

    def __init__(self, path: Path) -> None:
        self.path = path
        self.database = ladybug.Database(str(path), buffer_pool_size=128 * 1024 * 1024)
        self.connection = ladybug.Connection(self.database)
        self.initialize()

    def initialize(self) -> None:
        self.connection.execute(
            "CREATE NODE TABLE IF NOT EXISTS Entity("
            "stable_id STRING PRIMARY KEY, kind STRING, name STRING, body STRING, schema_version INT64)"
        )
        self.connection.execute(
            "CREATE REL TABLE IF NOT EXISTS REL("
            "FROM Entity TO Entity, stable_id STRING, kind STRING, position INT64)"
        )

    @staticmethod
    def _node_query() -> str:
        return (
            "MERGE (e:Entity {stable_id: $stable_id}) "
            "SET e.kind=$kind, e.name=$name, e.body=$body, e.schema_version=$schema_version"
        )

    @staticmethod
    def _edge_query() -> str:
        return (
            "MATCH (s:Entity {stable_id:$source}), (t:Entity {stable_id:$target}) "
            "MERGE (s)-[r:REL {stable_id:$stable_id}]->(t) "
            "SET r.kind=$kind, r.position=$position"
        )

    @staticmethod
    def _node_params(node: Node) -> dict[str, object]:
        return {
            "stable_id": node.stable_id,
            "kind": node.kind,
            "name": node.name,
            "body": node.body,
            "schema_version": node.schema_version,
        }

    @staticmethod
    def _edge_params(edge: Edge) -> dict[str, object]:
        return {
            "stable_id": edge.stable_id,
            "kind": edge.kind,
            "source": edge.source,
            "target": edge.target,
            "position": edge.position,
        }

    def transactional_seed(self, nodes: Iterable[Node], edges: Iterable[Edge]) -> None:
        self.connection.execute("BEGIN TRANSACTION")
        try:
            for node in nodes:
                self.connection.execute(self._node_query(), self._node_params(node))
            for edge in edges:
                self.connection.execute(self._edge_query(), self._edge_params(edge))
            self.connection.execute("COMMIT")
        except Exception:
            self.connection.execute("ROLLBACK")
            raise

    def upsert_node(self, node: Node) -> None:
        self.connection.execute("BEGIN TRANSACTION")
        try:
            self.connection.execute(self._node_query(), self._node_params(node))
            self.connection.execute("COMMIT")
        except Exception:
            self.connection.execute("ROLLBACK")
            raise

    def upsert_edge(self, edge: Edge) -> None:
        self.connection.execute("BEGIN TRANSACTION")
        try:
            self.connection.execute(self._edge_query(), self._edge_params(edge))
            self.connection.execute("COMMIT")
        except Exception:
            self.connection.execute("ROLLBACK")
            raise

    def delete_edge(self, stable_id: str) -> None:
        self.connection.execute(
            "MATCH ()-[r:REL]->() WHERE r.stable_id=$stable_id DELETE r",
            {"stable_id": stable_id},
        )

    def delete_node(self, stable_id: str) -> None:
        self.connection.execute(
            "MATCH (e:Entity {stable_id:$stable_id}) DETACH DELETE e",
            {"stable_id": stable_id},
        )

    def traverse(self, stable_id: str, *, direction: str, kind: str, hops: int) -> list[str]:
        if direction not in {"out", "in"} or hops not in {1, 2}:
            raise ValueError("direction must be out/in and hops must be 1/2")
        arrow = "-[r{idx}:REL]->" if direction == "out" else "<-[r{idx}:REL]-"
        chain = "(s:Entity {stable_id:$stable_id})"
        predicates: list[str] = []
        for idx in range(1, hops + 1):
            chain += arrow.format(idx=idx) + ("(n:Entity)" if idx == hops else f"(m{idx}:Entity)")
            predicates.append(f"r{idx}.kind=$kind")
        query = f"MATCH {chain} WHERE {' AND '.join(predicates)} RETURN DISTINCT n.stable_id ORDER BY n.stable_id"
        result = self.connection.execute(query, {"stable_id": stable_id, "kind": kind})
        return [str(row[0]) for row in result.get_all()]

    def export(self) -> tuple[list[Node], list[Edge]]:
        node_rows = self.connection.execute(
            "MATCH (e:Entity) RETURN e.stable_id,e.kind,e.name,e.body,e.schema_version ORDER BY e.stable_id"
        ).get_all()
        edge_rows = self.connection.execute(
            "MATCH (s:Entity)-[r:REL]->(t:Entity) "
            "RETURN r.stable_id,r.kind,s.stable_id,t.stable_id,r.position ORDER BY r.stable_id"
        ).get_all()
        nodes = [
            Node(str(row[0]), str(row[1]), str(row[2]), str(row[3]), int(row[4]))
            for row in node_rows
        ]
        edges = [
            Edge(str(row[0]), str(row[1]), str(row[2]), str(row[3]), int(row[4]))
            for row in edge_rows
        ]
        return nodes, edges

    def checkpoint(self) -> None:
        self.connection.execute("CHECKPOINT")

    def new_connection(self) -> ladybug.Connection:
        return ladybug.Connection(self.database)

    def close(self) -> None:
        self.connection.close()
        self.database.close()
