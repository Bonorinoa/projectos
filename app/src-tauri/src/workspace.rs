//! Native writer session and editor records. No provider credentials live here.
use std::{collections::BTreeMap, path::Path};
use projectos_kernel::{document::DocumentStore, graph::{Edge, GraphEngine, Object, Relation},
    graph_memory::InMemoryGraphEngine, proposals::{self, Decision, Proposal, ProposalKind},
    provider::stable_hash, registration::{self, RegisteredProject}, selection::DocAnchor};
use serde::{Deserialize, Serialize};

pub fn now() -> u64 {
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap_or_default().as_secs()
}
fn err(e: impl std::fmt::Display) -> String { e.to_string() }

#[derive(Clone, Serialize)]
pub struct Document { pub rel_path: String, pub text: String, pub hash: String }
#[derive(Serialize)]
pub struct CanvasObject { pub id: String, pub kind: String, pub label: String,
    pub fields: BTreeMap<String,String>, pub x: f64, pub y: f64 }
#[derive(Serialize)]
pub struct CanvasEdge { pub from: String, pub to: String, pub rel: String }
#[derive(Serialize)]
pub struct WorkspaceView { pub name: String, pub folder: String, pub uuid: String,
    pub documents: Vec<String>, pub objects: Vec<CanvasObject>, pub edges: Vec<CanvasEdge>,
    pub receipts: Vec<serde_json::Value>, pub declared_type: String,
    pub software_receipts: Vec<SoftwareReceipt> }

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct SoftwareReceipt {
    pub id: String, pub at: u64, pub operation: String, pub project_uuid: String,
    pub run_id: String, pub base_head: String, pub review_hash: Option<String>,
    #[serde(default)] pub changed_paths: Vec<String>,
    #[serde(default)] pub capability_scope: Vec<String>,
    pub outcome: String, pub failure: Option<String>,
}

#[derive(Clone, Serialize, Deserialize)]
pub struct Attempt {
    pub id: String, pub attempt_id: String, pub at: u64, pub op: String,
    pub rel_path: String, pub outcome: String, pub content_hash: String,
    pub requested_model: String, pub resolved_model: Option<String>,
    #[serde(default)] pub generation_id: Option<String>,
    pub usage_tokens: Option<u64>, pub estimated_upper_usd: f64,
    pub provider_cost_usd: Option<f64>, pub billed_usd: Option<f64>,
    pub cost_state: String, pub route: String, pub cost: Option<String>,
    pub requires_reconciliation: bool,
}
#[derive(Default, Serialize, Deserialize)]
struct Attempts { #[serde(default)] attempts: Vec<Attempt> }
#[derive(Default, Serialize, Deserialize)]
struct SoftwareReceipts { #[serde(default)] receipts: Vec<SoftwareReceipt> }
#[derive(Serialize, Deserialize)]
struct Snapshot { objects: BTreeMap<String,Object>, edges: Vec<Edge> }
#[derive(Default, Serialize, Deserialize)]
struct ReceiptFile { #[serde(default)] receipts: Vec<projectos_kernel::receipts::RedactedReceipt> }
#[derive(Default, Serialize, Deserialize)]
struct Annotations { #[serde(default)] annotations: Vec<Annotation> }
#[derive(Default,Serialize,Deserialize)]
struct Milestones { #[serde(default)] milestones: Vec<projectos_kernel::milestone::Milestone> }
#[derive(Default,Serialize,Deserialize)]
struct JournalRecords { #[serde(default)] records: Vec<projectos_kernel::journal::Record> }
#[derive(Serialize, Deserialize)]
struct Annotation { id: String, rel_path: String, doc_hash: String, byte_start: usize,
    byte_end: usize, action: String, text: String, at: u64 }

pub struct Workspace {
    pub project: RegisteredProject,
    pub files: DocumentStore,
    pub graph: InMemoryGraphEngine,
    pub attempts: Vec<Attempt>,
    pub software_receipts: Vec<SoftwareReceipt>,
}
impl Workspace {
    pub fn open(folder: &Path, create: bool) -> Result<Self,String> {
        Self::open_typed(folder, create, if create { Some("publication") } else { None })
    }
    pub fn open_typed(folder: &Path, create: bool, project_type: Option<&str>) -> Result<Self,String> {
        let root = std::fs::canonicalize(folder).map_err(err)?;
        // Refuse a malicious sidecar before invoking the legacy registry.
        let files = DocumentStore::open(&root).map_err(err)?;
        let manifest=files.read_metadata("project.toml").map_err(err)?;
        let supplied = match project_type {
            Some(value @ ("publication" | "software")) => Some(value),
            Some(_) => return Err("Choose either Substack or Software as the project type.".into()),
            None => None,
        };
        let existing = manifest.as_ref().map(|text| toml::from_str::<projectos_kernel::sidecar::SidecarEnvelope>(text)
            .map_err(|_| "Project registration is unreadable; repair it before opening.".to_string())).transpose()?;
        if let (Some(envelope), Some(requested)) = (&existing, supplied) {
            if envelope.declared_type != requested {
                return Err(format!("This project is registered as {}; reopen it without changing its type.", envelope.declared_type));
            }
        }
        if create {
            if manifest.is_none() {
                let declared_type=supplied.ok_or("Choose Substack or Software before registering a new project.")?;
                let envelope=projectos_kernel::sidecar::SidecarEnvelope {
                    project_uuid:uuid::Uuid::new_v4(),declared_type:declared_type.into(),content_roots:vec![],
                    schema_version:projectos_kernel::sidecar::SCHEMA_VERSION,engine_version:"0.0.1".into(),export_version:"0.0.1".into(),checkpoint:None,
                };
                files.write_metadata("project.toml",&toml::to_string(&envelope).map_err(err)?).map_err(err)?;
            }
            // Existing-manifest registration only reads. The unsafe legacy new-manifest writer is bypassed.
            let declared_type=existing.as_ref().map(|e|e.declared_type.as_str()).or(supplied).ok_or("Project type is unavailable.")?;
            registration::register(&root, declared_type, vec![]).map_err(err)?;
        }
        let project = registration::open(&root).map_err(err)?;
        let mut graph = InMemoryGraphEngine::new();
        if let Some(text) = files.read_metadata("graph/snapshot.toml").map_err(err)? {
            let snapshot: Snapshot = toml::from_str(&text).map_err(|_| "Project graph is unreadable; repair it before editing.".to_string())?;
            for o in snapshot.objects.into_values() { graph.upsert_object(o).map_err(err)?; }
            for e in snapshot.edges { graph.add_edge(e).map_err(err)?; }
        }
        let attempts = match files.read_metadata("provider-attempts.toml").map_err(err)? {
            Some(text) => toml::from_str::<Attempts>(&text).map_err(|_| "Provider receipts are unreadable; repair them before sending.".to_string())?.attempts,
            None => vec![],
        };
        let software_receipts = match files.read_metadata("software-receipts.toml").map_err(err)? {
            Some(text) => toml::from_str::<SoftwareReceipts>(&text).map_err(|_| "Software receipts are unreadable; repair them before continuing.".to_string())?.receipts,
            None => vec![],
        };
        let mut workspace = Self { project, files, graph, attempts, software_receipts };
        for path in workspace.files.list_text_files().map_err(err)? { workspace.track_draft(&path)?; }
        workspace.persist_graph()?;
        Ok(workspace)
    }
    pub fn unresolved(&self) -> bool { self.attempts.iter().any(|r| r.requires_reconciliation) }
    pub fn view(&self) -> Result<WorkspaceView,String> {
        let objects = self.graph.objects().map_err(err)?.into_iter().enumerate().map(|(i,o)| {
            let x = o.fields.get("canvas_x").and_then(|v|v.parse().ok()).unwrap_or(170. + (i % 3) as f64 * 300.);
            let y = o.fields.get("canvas_y").and_then(|v|v.parse().ok()).unwrap_or(140. + (i / 3) as f64 * 190.);
            CanvasObject { id:o.id,kind:o.kind,label:o.label,fields:o.fields,x,y }
        }).collect();
        let edges = self.graph.edges().map_err(err)?.into_iter().map(|e|CanvasEdge {
            from:e.from,to:e.to,rel:relation_label(e.rel).into()
        }).collect();
        let mut receipts: Vec<serde_json::Value> = match self.files.read_metadata("receipts.toml").map_err(err)? {
            Some(text) => toml::from_str::<ReceiptFile>(&text).map_err(|_|"Project receipts are unreadable.".to_string())?.receipts.into_iter()
                .map(serde_json::to_value).collect::<Result<_,_>>().map_err(err)?,
            None => vec![],
        };
        for attempt in &self.attempts { receipts.push(serde_json::to_value(attempt).map_err(err)?); }
        for receipt in &self.software_receipts { receipts.push(serde_json::to_value(receipt).map_err(err)?); }
        receipts.sort_by_key(|r|r["at"].as_u64().unwrap_or_default());
        Ok(WorkspaceView {
            name: self.project.folder.file_name().unwrap_or_default().to_string_lossy().into(),
            folder:self.project.folder.to_string_lossy().into(), uuid:self.project.envelope.project_uuid.to_string(),
            documents:self.files.list_text_files().map_err(err)?, objects,edges,receipts,
            declared_type:self.project.envelope.declared_type.clone(), software_receipts:self.software_receipts.clone(),
        })
    }
    pub fn read(&self, rel: &str) -> Result<Document,String> {
        let text = self.files.read(rel).map_err(err)?;
        Ok(Document{rel_path:rel.into(),hash:stable_hash(text.as_bytes()),text})
    }
    pub fn create(&mut self, rel: &str) -> Result<Document,String> {
        self.files.create(rel).map_err(err)?;
        self.track_draft(rel)?;
        self.persist_graph()?;
        self.read(rel)
    }
    pub fn anchor(&self, rel: &str, expected: &str, start: usize, end: usize) -> Result<(DocAnchor,String),String> {
        let doc = self.read(rel)?;
        if doc.hash != expected { return Err("The draft changed on disk. Reload and review the new text.".into()); }
        if start > end || end > doc.text.len() || !doc.text.is_char_boundary(start) || !doc.text.is_char_boundary(end) {
            return Err("Select a valid passage in the saved draft.".into());
        }
        let selected=doc.text[start..end].to_string();
        Ok((DocAnchor {rel_path:rel.into(),byte_start:start,byte_end:end,
            text_hash:stable_hash(selected.as_bytes()),doc_hash:doc.hash},selected))
    }
    pub fn save(&mut self, rel: &str, expected: &str, text: &str) -> Result<Document,String> {
        if text.len()>1_048_576 { return Err("Drafts are limited to 1 MiB.".into()); }
        let old = self.read(rel)?;
        let (anchor,_) = self.anchor(rel,expected,0,old.text.len())?;
        if old.text == text { return Ok(old); }
        let p=Proposal::new(rel,ProposalKind::Replacement,&old.text,text,expected);
        self.decide(&anchor,&p,"Save",true)?;
        self.read(rel)
    }
    pub fn decide(&mut self, anchor: &DocAnchor, p: &Proposal, action: &str, accept: bool) -> Result<(),String> {
        // The proposal and anchor are native-owned, never reconstructed from renderer output.
        if accept { self.anchor(&anchor.rel_path,&anchor.doc_hash,anchor.byte_start,anchor.byte_end)?; }
        let annotation_record = if accept && p.kind==ProposalKind::Annotation {
            let mut annotations=match self.files.read_metadata("annotations.toml").map_err(err)? {
                Some(t)=>toml::from_str::<Annotations>(&t).map_err(|_|"Annotations are unreadable.".to_string())?,
                None=>Annotations::default(),
            };
            let id=uuid::Uuid::new_v4().to_string();
            annotations.annotations.push(Annotation { id:id.clone(),rel_path:p.rel_path.clone(),doc_hash:anchor.doc_hash.clone(),
                byte_start:anchor.byte_start,byte_end:anchor.byte_end,action:action.into(),text:p.proposed.clone(),at:now() });
            let serialized=toml::to_string(&annotations).map_err(err)?;
            if serialized.len()>16*1024*1024 {return Err("Annotation storage is full. Export or archive annotations before accepting more.".into());}
            Some((id,serialized))
        } else { None };
        let revision = proposals::apply(&self.project.folder,anchor,p,if accept {Decision::Accept} else {Decision::Reject}).map_err(err)?;
        if accept {
            let draft = draft_id(&p.rel_path);
            self.track_draft(&p.rel_path)?;
            if p.kind==ProposalKind::Annotation {
                let (id,serialized)=annotation_record.expect("annotation preflight completed");
                self.files.write_metadata("annotations.toml",&serialized).map_err(err)?;
                self.graph.upsert_object(Object {id:id.clone(),kind:"Evidence".into(),label:format!("{action} · {}",p.rel_path),
                    fields:BTreeMap::from([("path".into(),p.rel_path.clone()),("source_hash".into(),anchor.doc_hash.clone()),
                        ("annotation_id".into(),id.clone()),("status".into(),"accepted annotation".into())]) }).map_err(err)?;
                self.graph.add_edge(Edge {from:id,to:draft,rel:Relation::Supports}).map_err(err)?;
            } else if let Some(rev)=revision {
                self.graph.upsert_object(Object {id:rev.id.clone(),kind:"Revision".into(),label:format!("{action} · {}",p.rel_path),
                    fields:BTreeMap::from([("path".into(),p.rel_path.clone()),("before".into(),rev.before_hash),
                        ("after".into(),rev.after_hash), ("status".into(),"accepted".into())])}).map_err(err)?;
                self.graph.add_edge(Edge {from:rev.id,to:draft,rel:Relation::RevisionOf}).map_err(err)?;
            }
        }
        self.persist_graph()
    }
    pub fn annotation(&self, id: &str) -> Result<String,String> {
        let text=self.files.read_metadata("annotations.toml").map_err(err)?.ok_or("No annotation found.")?;
        let items: Annotations=toml::from_str(&text).map_err(|_|"Annotations are unreadable.")?;
        items.annotations.into_iter().find(|a|a.id==id).map(|a|a.text).ok_or("No annotation found.".into())
    }
    pub fn record_attempt(&mut self, attempt: Attempt) -> Result<(),String> {
        self.track_draft(&attempt.rel_path)?;
        let recorded_id=attempt.id.clone();
        if let Some(old)=self.attempts.iter_mut().find(|r|r.id==attempt.id) { *old=attempt; } else {self.attempts.push(attempt);}
        self.files.write_metadata("provider-attempts.toml",&toml::to_string(&Attempts { attempts:self.attempts.clone() }).map_err(err)?).map_err(err)?;
        let r=self.attempts.iter().find(|r|r.id==recorded_id).ok_or("No attempt recorded.")?;
        self.graph.upsert_object(Object {id:r.id.clone(),kind:"Receipt".into(),label:format!("{} · {}",r.op,r.outcome),fields:BTreeMap::from([
            ("path".into(),r.rel_path.clone()),("route".into(),r.requested_model.clone()),("outcome".into(),r.outcome.clone()),
            ("cost".into(),r.cost.clone().unwrap_or_else(||"unknown".into()))])}).map_err(err)?;
        self.graph.add_edge(Edge {from:draft_id(&r.rel_path),to:r.id.clone(),rel:Relation::Produced}).map_err(err)?;
        self.persist_graph()
    }
    pub fn record_software_receipt(&mut self, mut receipt: SoftwareReceipt) -> Result<(),String> {
        if receipt.project_uuid != self.project.envelope.project_uuid.to_string() { return Err("Software receipt project mismatch.".into()); }
        if receipt.changed_paths.iter().any(|path| Path::new(path).is_absolute() || path.contains("..") || path.contains('\\')) {
            return Err("Software receipt contains a non-relative changed path.".into());
        }
        if receipt.failure.as_deref().is_some_and(|failure| failure.contains('/') || failure.contains('\\')) {
            return Err("Software receipt failure text may not contain machine paths.".into());
        }
        receipt.changed_paths.sort(); receipt.changed_paths.dedup();
        let id=receipt.id.clone();
        if let Some(old)=self.software_receipts.iter_mut().find(|r|r.id==id) { *old=receipt; } else { self.software_receipts.push(receipt); }
        self.files.write_metadata("software-receipts.toml", &toml::to_string(&SoftwareReceipts { receipts:self.software_receipts.clone() }).map_err(err)?).map_err(err)?;
        let saved=self.software_receipts.iter().find(|r|r.id==id).ok_or("No Software receipt recorded.")?;
        self.graph.upsert_object(Object { id:saved.id.clone(), kind:"Receipt".into(), label:format!("{} · {}",saved.operation,saved.outcome), fields:BTreeMap::from([
            ("operation".into(),saved.operation.clone()), ("outcome".into(),saved.outcome.clone()),
            ("base_head".into(),saved.base_head.clone()), ("review_hash".into(),saved.review_hash.clone().unwrap_or_default()),
            ("changed_paths".into(),saved.changed_paths.join(", ")),
        ])}).map_err(err)?;
        self.persist_graph()
    }
    pub fn milestone(&mut self,name:&str) -> Result<String,String> {
        if name.trim().is_empty() || name.len()>200 { return Err("Name the milestone (up to 200 characters).".into()); }
        let mut milestones:Milestones=match self.files.read_metadata("milestones.toml").map_err(err)? {
            Some(t)=>toml::from_str(&t).map_err(|_|"Milestones are unreadable.".to_string())?,None=>Milestones::default(),
        };
        let mut journal:JournalRecords=match self.files.read_metadata("journal.toml").map_err(err)? {
            Some(t)=>toml::from_str(&t).map_err(|_|"Journal is unreadable.".to_string())?,None=>JournalRecords::default(),
        };
        let checkpoint=uuid::Uuid::new_v4().to_string(); let at=now();
        milestones.milestones.push(projectos_kernel::milestone::Milestone { name:name.into(),checkpoint:checkpoint.clone(),at });
        journal.records.push(projectos_kernel::journal::Record::Checkpoint {id:checkpoint,at});
        self.persist_graph()?;
        self.files.write_metadata("journal.toml",&toml::to_string(&journal).map_err(err)?).map_err(err)?;
        self.files.write_metadata("milestones.toml",&toml::to_string(&milestones).map_err(err)?).map_err(err)?;
        Ok(name.into())
    }
    fn track_draft(&mut self,path:&str) -> Result<(),String> {
        let id=draft_id(path);
        let mut fields=self.graph.object(&id).map_err(err)?.map(|o|o.fields).unwrap_or_default();
        fields.insert("path".into(),path.into()); fields.insert("state".into(),"saved draft".into());
        self.graph.upsert_object(Object {id,kind:"Draft".into(),label:path.into(),fields}).map_err(err)
    }
    pub fn persist_graph(&self) -> Result<(),String> {
        let snapshot=Snapshot {objects:self.graph.objects().map_err(err)?.into_iter().map(|o|(o.id.clone(),o)).collect(),edges:self.graph.edges().map_err(err)?};
        self.files.write_metadata("graph/snapshot.toml",&toml::to_string(&snapshot).map_err(err)?).map_err(err)
    }
}
fn draft_id(path:&str) -> String { format!("draft-{}",stable_hash(path.as_bytes())) }
fn relation_label(r:Relation)-> &'static str { match r {
    Relation::Supports=>"SUPPORTS",Relation::Cites=>"CITES",Relation::GovernedBy=>"GOVERNED_BY",
    Relation::Executed=>"EXECUTED",Relation::Produced=>"PRODUCED",Relation::RevisionOf=>"REVISION_OF",
    Relation::AppliedTo=>"APPLIED_TO",Relation::LinksTo=>"LINKS_TO",
} }

#[cfg(test)]
mod tests {
    use super::*;
    fn software_receipt(project_uuid:String) -> SoftwareReceipt {
        SoftwareReceipt { id:"software-receipt-test".into(), at:now(), operation:"ReviewPatch".into(), project_uuid,
            run_id:"run-test".into(), base_head:"0123456789012345678901234567890123456789".into(), review_hash:Some("review-hash".into()),
            changed_paths:vec!["drafts/change.md".into()], capability_scope:vec!["staged.read".into(),"diff.generate".into()],
            outcome:"reviewed".into(), failure:None }
    }
    #[test]
    fn real_editor_save_reopen_stale_and_annotation() {
        let dir=tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("draft.md"),"Hello 🌍\n").unwrap();
        let mut w=Workspace::open(dir.path(),true).unwrap();
        let old=w.read("draft.md").unwrap();
        let saved=w.save("draft.md",&old.hash,"Changed 🌍\n").unwrap();
        assert_eq!(saved.text,"Changed 🌍\n");
        assert!(w.save("draft.md",&old.hash,"stale").is_err());
        let (a,text)=w.anchor("draft.md",&saved.hash,0,saved.text.len()).unwrap();
        let p=Proposal::new("draft.md",ProposalKind::Annotation,&text,"A claim needs evidence.",&saved.hash);
        w.decide(&a,&p,"FactCheck",true).unwrap();
        assert_eq!(w.read("draft.md").unwrap().text,saved.text);
        assert!(w.view().unwrap().objects.iter().any(|o|o.kind=="Evidence"));
        drop(w);
        let reopened=Workspace::open(dir.path(),false).unwrap();
        assert_eq!(reopened.read("draft.md").unwrap().text,saved.text);
        assert!(reopened.view().unwrap().objects.iter().any(|o|o.kind=="Revision"));
    }
    #[test]
    fn session_really_holds_writer_lock_and_receipts_are_real() {
        let dir=tempfile::tempdir().unwrap();
        let mut w=Workspace::open(dir.path(),true).unwrap();
        assert!(Workspace::open(dir.path(),false).is_err());
        assert!(w.view().unwrap().receipts.is_empty());
        w.create("draft.md").unwrap();
        assert!(w.create("draft.md").is_err());
        let old=w.read("draft.md").unwrap(); w.save("draft.md",&old.hash,"text").unwrap();
        assert!(!w.view().unwrap().receipts.is_empty());
        drop(w);
        Workspace::open(dir.path(),false).unwrap();
    }
    #[test]
    fn registration_and_milestones_ignore_preplanted_legacy_temp_symlinks() {
        use std::os::unix::fs::symlink;
        let dir=tempfile::tempdir().unwrap();let outside=tempfile::tempdir().unwrap();
        let victim=outside.path().join("victim");std::fs::write(&victim,"preserve").unwrap();
        let sidecar=dir.path().join(".projectos");std::fs::create_dir_all(sidecar.join("graph")).unwrap();
        for rel in ["project.toml.tmp","journal.toml.tmp","milestones.tmp","graph/snapshot.tmp"] {symlink(&victim,sidecar.join(rel)).unwrap();}
        let mut w=Workspace::open(dir.path(),true).unwrap();w.milestone("safe checkpoint").unwrap();
        assert_eq!(std::fs::read_to_string(&victim).unwrap(),"preserve");
        assert!(w.files.read_metadata("milestones.toml").unwrap().unwrap().contains("safe checkpoint"));
    }
    #[test]
    fn receipts_track_external_new_drafts_and_survive_reopening() {
        let dir=tempfile::tempdir().unwrap();let mut w=Workspace::open(dir.path(),true).unwrap();
        std::fs::write(dir.path().join("new.md"),"new").unwrap();
        let r=Attempt {id:"test-attempt".into(),attempt_id:"test-attempt".into(),at:now(),op:"Rephrase".into(),rel_path:"new.md".into(),outcome:"in-flight".into(),content_hash:stable_hash(b"new"),requested_model:"test/model".into(),resolved_model:None,generation_id:None,usage_tokens:None,estimated_upper_usd:0.0,provider_cost_usd:None,billed_usd:None,cost_state:"unknown".into(),route:"test/model".into(),cost:None,requires_reconciliation:true};
        w.record_attempt(r).unwrap();assert!(w.unresolved());
        assert!(w.view().unwrap().objects.iter().any(|o|o.kind=="Draft"&&o.label=="new.md"));
        drop(w);let reopened=Workspace::open(dir.path(),false).unwrap();assert!(reopened.unresolved());
        let json=serde_json::to_string(&reopened.view().unwrap().receipts).unwrap();assert!(!json.contains(dir.path().to_str().unwrap()));
    }
    #[test]
    fn typed_registration_is_authoritative_and_mismatches_are_refused() {
        let dir=tempfile::tempdir().unwrap();
        let w=Workspace::open_typed(dir.path(),true,Some("software")).unwrap();
        assert_eq!(w.view().unwrap().declared_type,"software");
        drop(w);
        assert!(Workspace::open_typed(dir.path(),false,Some("publication")).err().unwrap().contains("registered as software"));
        let reopened=Workspace::open_typed(dir.path(),false,None).unwrap();
        assert_eq!(reopened.project.envelope.declared_type,"software");
    }
    #[test]
    fn software_receipts_round_trip_without_absolute_machine_paths() {
        let dir=tempfile::tempdir().unwrap();let mut w=Workspace::open_typed(dir.path(),true,Some("software")).unwrap();
        let receipt=software_receipt(w.project.envelope.project_uuid.to_string());
        w.record_software_receipt(receipt).unwrap();
        let serialized=w.files.read_metadata("software-receipts.toml").unwrap().unwrap();
        assert!(!serialized.contains(dir.path().to_str().unwrap()));
        assert!(serialized.contains("drafts/change.md"));
        drop(w);let reopened=Workspace::open_typed(dir.path(),false,None).unwrap();
        assert_eq!(reopened.view().unwrap().software_receipts.len(),1);
        assert!(reopened.view().unwrap().receipts.iter().any(|value|value["operation"]=="ReviewPatch"));
    }
    #[test]
    fn software_receipt_persistence_failure_is_reported_and_paths_are_rejected() {
        let dir=tempfile::tempdir().unwrap();let mut w=Workspace::open_typed(dir.path(),true,Some("software")).unwrap();
        let mut leaking=software_receipt(w.project.envelope.project_uuid.to_string());
        leaking.changed_paths=vec![dir.path().join("secret").to_string_lossy().into_owned()];
        assert!(w.record_software_receipt(leaking).unwrap_err().contains("non-relative"));
        std::fs::create_dir(dir.path().join(".projectos/software-receipts.toml")).unwrap();
        let receipt=software_receipt(w.project.envelope.project_uuid.to_string());
        assert!(w.record_software_receipt(receipt).is_err());
    }
}
