use helix_db::{Client, DbConfig, HelixDbSource};
use helix_core_db::config::QueryTelemetry;
use serde_json::{json, Value};
use std::collections::BTreeMap;

async fn query(client: &Client, text: &str) -> Result<Value, String> {
    let result = client.cypher(text, BTreeMap::new(), None).await.map_err(|e| e.to_string())?;
    Ok(json!({"columns": result.columns, "rows": result.rows}))
}

fn local_only_config() -> DbConfig {
    // The upstream default consults ~/.helix and may start network telemetry.
    // Disable before opening storage so neither settings nor transport starts.
    DbConfig::new().with_query_telemetry(QueryTelemetry::Disabled)
}

// No caller-supplied path, query, URL or credentials. This only touches its own
// disposable test store and returns a bounded receipt to the frontend.
async fn contract() -> Result<Value, String> {
    let temp = tempfile::tempdir().map_err(|e| e.to_string())?;
    let source = HelixDbSource::Disk { root: temp.path().to_path_buf(), database: "projectos-gate-b".into() };
    let client = Client::open_with_config(source.clone(), local_only_config()).await.map_err(|e| e.to_string())?;
    query(&client, "CREATE (a:Artifact {sid: 'a'}), (b:Artifact {sid: 'b'}), (c:Artifact {sid: 'c'}), (a)-[:DERIVES]->(b), (b)-[:DERIVES]->(c)").await?;
    let one = query(&client, "MATCH (a:Artifact {sid: 'a'})-[:DERIVES]->(b) RETURN b.sid ORDER BY b.sid").await?;
    if one["rows"] != json!([["b"]]) { return Err(format!("one-hop mismatch: {one}")); }
    let two = query(&client, "MATCH (a:Artifact {sid: 'a'})-[:DERIVES]->(b)-[:DERIVES]->(c) RETURN c.sid").await?;
    if two["rows"] != json!([["c"]]) { return Err(format!("two-hop mismatch: {two}")); }
    query(&client, "MATCH (a:Artifact {sid: 'a'}) SET a.title = 'updated'").await?;
    let updated = query(&client, "MATCH (a:Artifact {sid: 'a'}) RETURN a.title").await?;
    if updated["rows"] != json!([["updated"]]) { return Err("update mismatch".into()); }
    query(&client, "MATCH (c:Artifact {sid: 'c'}) DETACH DELETE c").await?;
    let exported = query(&client, "MATCH (a:Artifact) RETURN a.sid ORDER BY a.sid").await?;
    if exported["rows"] != json!([["a"], ["b"]]) { return Err(format!("delete mismatch: {exported}")); }
    client.close().await.map_err(|e| e.to_string())?;
    let reopened = Client::open_with_config(source.clone(), local_only_config()).await.map_err(|e| e.to_string())?;
    let persisted = query(&reopened, "MATCH (a:Artifact) RETURN a.sid ORDER BY a.sid").await?;
    if persisted != exported { return Err("reopen mismatch".into()); }
    reopened.close().await.map_err(|e| e.to_string())?;
    let reader = Client::open_reader_with_config(source, local_only_config()).await.map_err(|e| e.to_string())?;
    let readonly = query(&reader, "CREATE (:Artifact {sid: 'denied'})").await.is_err();
    reader.close().await.map_err(|e| e.to_string())?;
    if !readonly { return Err("read-only handle allowed mutation".into()); }
    Ok(json!({"status":"pass", "engine":"Helix", "sourceRevision":"c72e64f0c7e480f3d125f071659d5e3ad7b8da15", "checks":6, "mode":"in-process Rust", "queryTelemetry":"explicitly-disabled", "operations":["one-hop","two-hop","update","delete","disk-reopen","read-only write rejection"], "export":exported}))
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn local_only_config_disables_upstream_telemetry_before_any_open() {
        assert_eq!(local_only_config().query_telemetry(), QueryTelemetry::Disabled);
    }
    #[tokio::test]
    async fn local_contract_still_passes_with_explicitly_disabled_telemetry() {
        let receipt = contract().await.unwrap();
        assert_eq!(receipt["status"], "pass");
        assert_eq!(receipt["queryTelemetry"], "explicitly-disabled");
    }
}

#[cfg(feature = "desktop")]
#[tauri::command]
async fn graph_probe() -> Result<Value, String> { contract().await }

fn main() {
    if std::env::args().any(|arg| arg == "--contract") {
        let runtime = tokio::runtime::Runtime::new().unwrap();
        match runtime.block_on(contract()) {
            Ok(receipt) => println!("{}", receipt),
            Err(error) => { eprintln!("{}", json!({"status":"fail","error":error})); std::process::exit(1); }
        }
        return;
    }
    #[cfg(feature = "desktop")]
    tauri::Builder::default().invoke_handler(tauri::generate_handler![graph_probe])
        .run(tauri::generate_context!()).expect("graph embedding shell failed");
    #[cfg(not(feature = "desktop"))]
    { eprintln!("use --contract or build with --features desktop"); std::process::exit(2); }
}
