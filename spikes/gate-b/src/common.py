from __future__ import annotations

import hashlib
import json
import sqlite3
from dataclasses import dataclass
from pathlib import Path
from typing import Any, Iterable


@dataclass(frozen=True)
class Node:
    stable_id: str
    kind: str
    name: str
    body: str
    schema_version: int = 1


@dataclass(frozen=True)
class Edge:
    stable_id: str
    kind: str
    source: str
    target: str
    position: int = 0


def load_corpus(path: Path) -> tuple[list[Node], list[Edge]]:
    raw = json.loads(path.read_text(encoding="utf-8"))
    schema_version = int(raw["schema_version"])
    nodes = [Node(schema_version=schema_version, **item) for item in raw["nodes"]]
    edges = [Edge(**item) for item in raw["edges"]]
    return nodes, edges


def canonical_records(nodes: Iterable[Node], edges: Iterable[Edge], *, export_version: int = 1) -> list[dict[str, Any]]:
    node_rows = [
        {
            "record": "node",
            "stable_id": n.stable_id,
            "kind": n.kind,
            "name": n.name,
            "body": n.body,
            "schema_version": n.schema_version,
        }
        for n in nodes
    ]
    edge_rows = [
        {
            "record": "edge",
            "stable_id": e.stable_id,
            "kind": e.kind,
            "source": e.source,
            "target": e.target,
            "position": e.position,
        }
        for e in edges
    ]
    return [
        {"record": "header", "export_version": export_version, "encoding": "utf-8-jsonl"},
        *sorted(node_rows, key=lambda row: row["stable_id"]),
        *sorted(edge_rows, key=lambda row: row["stable_id"]),
    ]


def canonical_bytes(nodes: Iterable[Node], edges: Iterable[Edge], *, export_version: int = 1) -> bytes:
    lines = [
        json.dumps(row, ensure_ascii=False, sort_keys=True, separators=(",", ":"))
        for row in canonical_records(nodes, edges, export_version=export_version)
    ]
    return ("\n".join(lines) + "\n").encode("utf-8")


def canonical_hash(payload: bytes) -> str:
    return hashlib.sha256(payload).hexdigest()


def parse_canonical(payload: bytes) -> tuple[list[Node], list[Edge], int]:
    rows = [json.loads(line) for line in payload.decode("utf-8").splitlines() if line]
    if not rows or rows[0].get("record") != "header":
        raise ValueError("canonical export is missing its header")
    version = int(rows[0]["export_version"])
    nodes = [
        Node(
            stable_id=row["stable_id"],
            kind=row["kind"],
            name=row["name"],
            body=row["body"],
            schema_version=int(row["schema_version"]),
        )
        for row in rows[1:]
        if row.get("record") == "node"
    ]
    edges = [
        Edge(
            stable_id=row["stable_id"],
            kind=row["kind"],
            source=row["source"],
            target=row["target"],
            position=int(row["position"]),
        )
        for row in rows[1:]
        if row.get("record") == "edge"
    ]
    return nodes, edges, version


def migrate_v1_to_v2(payload: bytes) -> bytes:
    nodes, edges, version = parse_canonical(payload)
    if version != 1:
        raise ValueError(f"expected export version 1, got {version}")
    migrated = [
        Node(
            stable_id=n.stable_id,
            kind=n.kind,
            name=n.name,
            body=n.body,
            schema_version=2,
        )
        for n in nodes
    ]
    return canonical_bytes(migrated, edges, export_version=2)


class CompanionFts:
    """Disposable FTS5 index whose only durable mapping is graph stable_id."""

    def __init__(self, path: Path) -> None:
        self.path = path
        self.connection = sqlite3.connect(path)
        self.connection.execute(
            "CREATE VIRTUAL TABLE IF NOT EXISTS entity_fts USING fts5(stable_id UNINDEXED, name, body)"
        )

    def rebuild(self, nodes: Iterable[Node]) -> None:
        with self.connection:
            self.connection.execute("DELETE FROM entity_fts")
            self.connection.executemany(
                "INSERT INTO entity_fts(stable_id, name, body) VALUES (?, ?, ?)",
                [(n.stable_id, n.name, n.body) for n in nodes],
            )

    def search(self, query: str, limit: int = 10) -> list[str]:
        rows = self.connection.execute(
            "SELECT stable_id FROM entity_fts WHERE entity_fts MATCH ? ORDER BY rank, stable_id LIMIT ?",
            (query, limit),
        ).fetchall()
        return [str(row[0]) for row in rows]

    def close(self) -> None:
        self.connection.close()


def append_journal(path: Path, record: dict[str, Any]) -> None:
    path.parent.mkdir(parents=True, exist_ok=True)
    encoded = json.dumps(record, sort_keys=True, separators=(",", ":")) + "\n"
    with path.open("a", encoding="utf-8") as handle:
        handle.write(encoded)
        handle.flush()
        import os

        os.fsync(handle.fileno())


def read_journal(path: Path) -> list[dict[str, Any]]:
    return [json.loads(line) for line in path.read_text(encoding="utf-8").splitlines() if line]
