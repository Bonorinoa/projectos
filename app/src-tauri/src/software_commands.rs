//! Local Software commands. The renderer supplies native-owned ids, never Git argv.
use super::{idle, locked, AppState, Inner};
use super::software::{SoftwareBroker, RepositoryInfo, WorktreeInfo, PatchReview};
use super::workspace::{now, SoftwareReceipt};
use tauri::State;

const CREATE_SCOPE: &[&str] = &["canonical.read", "git.inspect", "git.worktree.create"];
const REVIEW_SCOPE: &[&str] = &["staged.read", "diff.generate", "hash.bind"];
const EXPORT_SCOPE: &[&str] = &["review.revalidate", "patch.export"];

fn record(inner: &mut Inner, operation: &str, run_id: &str, base_head: &str, review_hash: Option<&str>, changed_paths: Vec<String>, capability_scope: &[&str], outcome: &str, failure: Option<&str>) -> Result<(), String> {
    let project_uuid=inner.workspace.as_ref().ok_or("Project closed.")?.project.envelope.project_uuid.to_string();
    inner.workspace.as_mut().ok_or("Project closed.")?.record_software_receipt(SoftwareReceipt {
        id:format!("software-{}",uuid::Uuid::new_v4()), at:now(), operation:operation.into(), project_uuid,
        run_id:run_id.into(), base_head:base_head.into(), review_hash:review_hash.map(str::to_string),
        changed_paths, capability_scope:capability_scope.iter().map(|value|(*value).into()).collect(),
        outcome:outcome.into(), failure:failure.map(str::to_string),
    })
}

fn broker(inner: &mut Inner) -> Result<&mut SoftwareBroker, String> {
    idle(inner)?;
    if inner.software_receipt_blocked { return Err("A prior Software effect could not be receipted. Reopen the project and inspect the staging/export result before continuing.".into()); }
    if !inner.proposals.is_empty() { return Err("Review the pending AI proposal first.".into()); }
    if inner.software.is_none() {
        let workspace=inner.workspace.as_ref().ok_or("Open a project first.")?;
        if workspace.project.envelope.declared_type != "software" { return Err("Software tools are available only for projects registered as Software.".into()); }
        let root = workspace.project.folder.clone();
        let temp = std::fs::canonicalize(std::env::temp_dir()).map_err(|_| "Staging storage is unavailable.")?;
        let parent = temp.join(format!("projectos-worktrees-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir(&parent).map_err(|_| "Could not create isolated staging storage.")?;
        #[cfg(unix)] {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(&parent, std::fs::Permissions::from_mode(0o700)).map_err(|_| "Could not protect staging storage.")?;
        }
        match SoftwareBroker::new(&root, &parent) {
            Ok(value) => inner.software = Some(value),
            Err(error) => { let _ = std::fs::remove_dir(&parent); return Err(error); }
        }
    }
    inner.software.as_mut().ok_or("Software session is unavailable.".into())
}

#[tauri::command]
pub fn software_status(state: State<AppState>) -> Result<RepositoryInfo, String> {
    let mut inner=locked(&state)?;
    broker(&mut inner)?.inspect()
}

#[tauri::command]
pub fn software_create_worktree(confirmed: bool, state: State<AppState>) -> Result<WorktreeInfo, String> {
    let mut inner = locked(&state)?;
    let run_id=uuid::Uuid::new_v4().to_string();
    let head=broker(&mut inner)?.inspect()?.head;
    if !confirmed {
        record(&mut inner,"CreateWorktree",&run_id,&head,None,vec![],CREATE_SCOPE,"refused",Some("explicit confirmation missing"))?;
        return Err("Confirm isolated worktree creation first.".into());
    }
    let stage = match broker(&mut inner)?.create_worktree(true) {
        Ok(stage)=>stage,
        Err(error)=>{record(&mut inner,"CreateWorktree",&run_id,&head,None,vec![],CREATE_SCOPE,"refused",Some("worktree creation refused"))?;return Err(error)}
    };
    if let Err(error) = record(&mut inner,"CreateWorktree",&stage.id,&stage.base_head,None,vec![],CREATE_SCOPE,"staged",None) {
        inner.software_receipt_blocked=true;
        return Err(format!("Worktree created at {}, but its receipt could not be saved: {error}. Do not repeat creation blindly.", stage.path));
    }
    Ok(stage)
}

#[tauri::command]
pub fn software_review(worktree_id: String, state: State<AppState>) -> Result<PatchReview, String> {
    let mut inner = locked(&state)?;
    let head=broker(&mut inner)?.inspect()?.head;
    let review = match broker(&mut inner)?.review(&worktree_id) {
        Ok(review)=>review,
        Err(error)=>{record(&mut inner,"ReviewPatch",&worktree_id,&head,None,vec![],REVIEW_SCOPE,"refused",Some("patch review refused"))?;return Err(error)}
    };
    let paths=review.files.iter().map(|file|file.path.clone()).collect();
    record(&mut inner,"ReviewPatch",&review.worktree_id,&review.base_head,Some(&review.content_hash),paths,REVIEW_SCOPE,"reviewed",None)?;
    Ok(review)
}

#[tauri::command]
pub async fn software_export_review(review_id: String, content_hash: String, state: State<'_,AppState>) -> Result<Option<String>, String> {
    let run_id=uuid::Uuid::new_v4().to_string();
    let (project_uuid,binding) = {
        let mut inner = locked(&state)?;
        if let Err(error)=broker(&mut inner)?.export_review(&review_id, &content_hash) {
            let binding=broker(&mut inner)?.review_binding(&review_id).ok();
            let head=binding.as_ref().map(|review|review.base_head.clone()).unwrap_or_else(||"unknown".into());
            let run=binding.as_ref().map(|review|review.worktree_id.as_str()).unwrap_or(&run_id);
            let paths=binding.as_ref().map(|review|review.files.iter().map(|file|file.path.clone()).collect()).unwrap_or_default();
            record(&mut inner,"ExportPatch",run,&head,Some(&content_hash),paths,EXPORT_SCOPE,"refused",Some("review revalidation refused"))?;
            return Err(error)
        }
        let binding=broker(&mut inner)?.review_binding(&review_id)?;
        (inner.workspace.as_ref().ok_or("Project closed.")?.project.envelope.project_uuid,binding)
    };
    let destination = tauri::async_runtime::spawn_blocking(|| rfd::FileDialog::new().set_title("Export reviewed patch").set_file_name("reviewed.patch").save_file()).await.map_err(|_| "Patch export dialog could not open.")?;
    let Some(destination) = destination else {
        let mut inner=locked(&state)?;
        let paths=binding.files.iter().map(|file|file.path.clone()).collect();
        record(&mut inner,"ExportPatch",&binding.worktree_id,&binding.base_head,Some(&content_hash),paths,EXPORT_SCOPE,"cancelled",None)?;
        return Ok(None)
    };
    let mut inner = locked(&state)?;
    if inner.workspace.as_ref().map(|w| w.project.envelope.project_uuid) != Some(project_uuid) { return Err("The project changed. Review this patch again.".into()); }
    // Revalidate after the dialog: external changes while it was open invalidate export.
    export_to(&mut inner,&binding,&content_hash,&destination)?;
    Ok(Some(destination.to_string_lossy().into_owned()))
}

/// Native-owned export path, shared by the dialog command and fault-injection tests.
/// This helper is deliberately not exposed as a Tauri command.
pub(crate) fn export_to(inner:&mut Inner,binding:&PatchReview,content_hash:&str,destination:&std::path::Path)->Result<(),String> {
    let paths=binding.files.iter().map(|file|file.path.clone()).collect::<Vec<_>>();
    let patch=match broker(inner)?.export_review(&binding.id,content_hash) {
        Ok(patch)=>patch,
        Err(error)=>{record(inner,"ExportPatch",&binding.worktree_id,&binding.base_head,Some(content_hash),paths,EXPORT_SCOPE,"refused",Some("review revalidation refused"))?;return Err(error)}
    };
    if destination.extension().and_then(|s|s.to_str())!=Some("patch") {
        record(inner,"ExportPatch",&binding.worktree_id,&binding.base_head,Some(content_hash),paths,EXPORT_SCOPE,"refused",Some("patch extension required"))?;
        return Err("Choose a new file ending in .patch.".into())
    }
    // Never overwrite a source, existing export or a symlink through this operation.
    use std::io::Write;
    let mut file=match std::fs::OpenOptions::new().write(true).create_new(true).open(destination) {
        Ok(file)=>file,
        Err(_)=>{record(inner,"ExportPatch",&binding.worktree_id,&binding.base_head,Some(content_hash),paths,EXPORT_SCOPE,"refused",Some("destination unavailable or already exists"))?;return Err("Choose a new patch filename; existing files are preserved.".into())}
    };
    if file.write_all(patch.as_bytes()).and_then(|_|file.sync_all()).is_err() {
        inner.software_receipt_blocked=true;
        let receipt=record(inner,"ExportPatch",&binding.worktree_id,&binding.base_head,Some(content_hash),paths,EXPORT_SCOPE,"outcome-unknown",Some("partial export requires inspection"));
        return Err(format!("Patch export may be partial; inspect the destination before continuing. Do not repeat export blindly.{}",if receipt.is_err(){" Its failure receipt could not be saved."}else{""}));
    }
    if let Err(error)=record(inner,"ExportPatch",&binding.worktree_id,&binding.base_head,Some(content_hash),paths,EXPORT_SCOPE,"exported",None) {
        inner.software_receipt_blocked=true;
        return Err(format!("Patch written, but its receipt could not be saved: {error}. Do not repeat export blindly."));
    }
    Ok(())
}
