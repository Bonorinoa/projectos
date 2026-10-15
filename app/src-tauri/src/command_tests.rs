//! Exercise the shipping command handler, serialization and managed writer state.
//! The mock replaces only the native window runtime; files and command code are real.
use super::*;
use serde_json::{json, Value};
use tauri::{test::{mock_builder, mock_context, noop_assets, MockRuntime}, Manager};

fn app() -> (tauri::App<MockRuntime>, tauri::WebviewWindow<MockRuntime>) {
    let app = app_builder(mock_builder()).build(mock_context(noop_assets())).unwrap();
    let window = tauri::WebviewWindowBuilder::new(&app, "main", Default::default()).build().unwrap();
    (app, window)
}

fn invoke(window: &tauri::WebviewWindow<MockRuntime>, command: &str, body: Value) -> Result<Value, Value> {
    tauri::test::get_ipc_response(window, tauri::webview::InvokeRequest {
        cmd: command.into(), callback: tauri::ipc::CallbackFn(0), error: tauri::ipc::CallbackFn(1),
        url: "tauri://localhost".parse().unwrap(), body: tauri::ipc::InvokeBody::Json(body),
        headers: Default::default(), invoke_key: tauri::test::INVOKE_KEY.into(),
    }).map(|body| body.deserialize::<Value>().unwrap())
}

fn open(window: &tauri::WebviewWindow<MockRuntime>, folder: &std::path::Path) -> Value {
    invoke(window, "workspace_open", json!({"folder": folder.to_str().unwrap(), "create": true, "projectType":"publication"})).unwrap()
}

#[test]
fn dispatcher_requires_and_binds_camel_case_project_type() {
    let root=tempfile::tempdir().unwrap();let (_app,window)=app();
    let missing=invoke(&window,"workspace_open",json!({"folder":root.path().to_str().unwrap(),"create":true})).unwrap_err();
    assert!(missing.as_str().unwrap().contains("Choose Substack or Software"));
    let opened=invoke(&window,"workspace_open",json!({"folder":root.path().to_str().unwrap(),"create":true,"projectType":"software"})).unwrap();
    assert_eq!(opened["declared_type"],"software");
    assert!(opened["software_receipts"].as_array().unwrap().is_empty());
    invoke(&window,"workspace_close",json!({})).unwrap();
    let mismatch=invoke(&window,"workspace_open",json!({"folder":root.path().to_str().unwrap(),"create":false,"projectType":"publication"})).unwrap_err();
    assert!(mismatch.as_str().unwrap().contains("registered as software"));
    let reopened=invoke(&window,"workspace_open",json!({"folder":root.path().to_str().unwrap(),"create":false})).unwrap();
    assert_eq!(reopened["declared_type"],"software");
}

#[test]
fn dispatcher_binds_editor_arguments_and_releases_the_writer_session() {
    let root = tempfile::tempdir().unwrap();
    std::fs::write(root.path().join("draft.md"), "A café draft 🌍\n").unwrap();
    let (_app, window) = app();
    let opened = open(&window, root.path());
    assert!(opened["uuid"].as_str().is_some());
    assert!(root.path().join(".projectos/lock").exists());
    let doc = invoke(&window, "draft_open", json!({"relPath":"draft.md"})).unwrap();
    assert_eq!(doc["text"], "A café draft 🌍\n");
    let saved = invoke(&window, "draft_save", json!({
        "relPath":"draft.md", "expectedHash":doc["hash"], "text":"Saved café 🌍\n"
    })).unwrap();
    assert_eq!(saved["text"], "Saved café 🌍\n");
    let stale = invoke(&window, "draft_save", json!({
        "relPath":"draft.md", "expectedHash":doc["hash"], "text":"should never land"
    })).unwrap_err();
    assert!(stale.as_str().unwrap().contains("changed"));
    assert_eq!(std::fs::read_to_string(root.path().join("draft.md")).unwrap(), "Saved café 🌍\n");
    let created = invoke(&window, "draft_create", json!({"relPath":"new.md"})).unwrap();
    assert_eq!(created["text"], "");
    assert!(invoke(&window, "draft_create", json!({"relPath":"new.md"})).is_err());
    assert!(invoke(&window, "draft_open", json!({"relPath":"../outside.md"})).is_err());
    let offline = invoke(&window, "action_prepare", json!({
        "relPath":"draft.md", "expectedHash":saved["hash"], "byteStart":0, "byteEnd":5,
        "action":"Rephrase", "model":"test/fixed", "maxTokens":64, "ceilingUsd":0.01
    })).unwrap_err();
    assert!(offline.as_str().unwrap().contains("Turn on live AI"));
    assert!(invoke(&window, "action_execute", json!({"preparedId":"unapproved"})).unwrap_err().as_str().unwrap().contains("off"));
    assert!(!root.path().join(".projectos/provider-attempts.toml").exists());
    invoke(&window, "workspace_close", json!({})).unwrap();
    assert!(!root.path().join(".projectos/lock").exists());
    assert!(invoke(&window, "draft_open", json!({"relPath":"draft.md"})).is_err());
    open(&window, root.path());
    assert_eq!(invoke(&window, "draft_open", json!({"relPath":"draft.md"})).unwrap()["text"], "Saved café 🌍\n");
}

fn pending(app: &tauri::App<MockRuntime>, id: &str, annotation: bool, proposed: &str) {
    let state = app.state::<AppState>();
    let mut inner = locked(&state).unwrap();
    let workspace = workspace_mut(&mut inner).unwrap();
    let doc = workspace.read("draft.md").unwrap();
    let (anchor, original) = workspace.anchor("draft.md", &doc.hash, 0, doc.text.len()).unwrap();
    let proposal = Proposal::new("draft.md", if annotation {ProposalKind::Annotation} else {ProposalKind::Replacement}, &original, proposed, &doc.hash);
    let result = ProposalResult {
        id:id.into(), action:if annotation {"Explain".into()} else {"Rephrase".into()},
        rel_path:"draft.md".into(), original, proposed:proposed.into(), annotation, truncated:false,
        requested_model:"test/fixed".into(), resolved_model:"test/fixed".into(), usage_tokens:None,
        provider_cost_usd:None, estimated_upper_usd:0.0, cost_state:"unknown".into(), attempt_id:"test-attempt".into(),
    };
    inner.proposals.insert(id.into(), Pending {anchor, proposal, result});
}

#[test]
fn dispatcher_reviews_native_proposals_and_can_reject_a_stale_result() {
    let root = tempfile::tempdir().unwrap();
    std::fs::write(root.path().join("draft.md"), "Original draft.\n").unwrap();
    let (app, window) = app();
    open(&window, root.path());
    pending(&app, "annotation", true, "An explanation of the passage.");
    let annotated = invoke(&window, "action_decide", json!({"proposalId":"annotation", "accept":true})).unwrap();
    assert_eq!(std::fs::read_to_string(root.path().join("draft.md")).unwrap(), "Original draft.\n");
    let annotation = annotated["objects"].as_array().unwrap().iter().find(|o|o["kind"]=="Evidence").unwrap();
    assert_eq!(invoke(&window, "artifact_annotation", json!({"annotationId":annotation["fields"]["annotation_id"]})).unwrap(), "An explanation of the passage.");
    pending(&app, "replacement", false, "Native approved text.\n");
    invoke(&window, "action_decide", json!({"proposalId":"replacement", "accept":true, "proposed":"FORGED RENDERER TEXT"})).unwrap();
    assert_eq!(std::fs::read_to_string(root.path().join("draft.md")).unwrap(), "Native approved text.\n");
    assert!(invoke(&window, "action_decide", json!({"proposalId":"replacement", "accept":true})).is_err());
    pending(&app, "stale", false, "A stale proposal.");
    std::fs::write(root.path().join("draft.md"), "External edit stays.\n").unwrap();
    assert!(invoke(&window, "action_decide", json!({"proposalId":"stale", "accept":true})).is_err());
    invoke(&window, "action_decide", json!({"proposalId":"stale", "accept":false})).unwrap();
    assert_eq!(std::fs::read_to_string(root.path().join("draft.md")).unwrap(), "External edit stays.\n");
    assert!(locked(&app.state::<AppState>()).unwrap().proposals.is_empty());
}

#[test]
fn dispatcher_confirms_exit_only_after_requests_and_proposals_are_reviewed() {
    let root=tempfile::tempdir().unwrap();
    std::fs::write(root.path().join("draft.md"),"A saved draft.\n").unwrap();
    let (app,window)=app();
    open(&window,root.path());
    pending(&app,"pending",false,"Review before quitting.");
    assert!(invoke(&window,"app_confirm_exit",json!({})).unwrap_err().as_str().unwrap().contains("Accept or reject"));
    assert!(!locked(&app.state::<AppState>()).unwrap().exit_approved);
    {
        let state=app.state::<AppState>();let mut inner=locked(&state).unwrap();
        inner.proposals.clear();let (cancel,_rx)=watch::channel(false);
        inner.active=Some(Active{id:"test-running".into(),cancel});
    }
    assert!(invoke(&window,"app_confirm_exit",json!({})).unwrap_err().as_str().unwrap().contains("Cancel or finish"));
    locked(&app.state::<AppState>()).unwrap().active=None;
    // MockRuntime's process-exit operation is unimplemented. The two refusal
    // paths above use real IPC; successful approval uses the same native policy
    // helper, with actual process exit covered by the desktop walkthrough.
    approve_exit(&app.state::<AppState>()).unwrap();
    assert!(locked(&app.state::<AppState>()).unwrap().exit_approved);
    assert!(!root.path().join(".projectos/lock").exists());
}
