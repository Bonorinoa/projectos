from pathlib import Path
from zipfile import ZipFile
from lxml import etree as E
import hashlib, json, shutil, importlib.util

BASE=Path('/private/tmp/projectos-system-design-docx')
REF=Path('/Users/bonorinoa/.codex/plugins/cache/openai-curated-remote/openai-templates/0.1.1/skills/artifact-template-system-design/assets/reference.docx')
OUT=Path('/Users/bonorinoa/Documents/ChatGPT/ProjectOS/docs/ProjectOS-M3-First-Project-System-Design.docx')
SKILL=Path('/Users/bonorinoa/.codex/plugins/cache/openai-primary-runtime/documents/26.1013.11914/skills/documents')
spec=importlib.util.spec_from_file_location('ooxml_patch',SKILL/'scripts/docx_ooxml_patch.py');patch=importlib.util.module_from_spec(spec);spec.loader.exec_module(patch)
assert hashlib.sha256(REF.read_bytes()).hexdigest()=='13504f6c221a42c1726460a9e865e563355539ff97d702d6c9b2267b4b261d76'
N={'w':patch.W_NS,'wp':'http://schemas.openxmlformats.org/drawingml/2006/wordprocessingDrawing'}
def tx(el):return ''.join(el.xpath('.//w:t/text()',namespaces=N))
with ZipFile(REF) as z:
    inventory={i.filename:{'size':i.file_size,'sha256':hashlib.sha256(z.read(i)).hexdigest()} for i in z.infolist()}
    tree=E.fromstring(z.read('word/document.xml'))
    inventory['structure']={'sections':len(tree.findall('.//w:sectPr',N)),'tables':len(tree.findall('.//w:tbl',N)),'controls':len(tree.findall('.//w:sdt',N)),'fields':tree.xpath('.//w:instrText/text()',namespaces=N),'body_slots':[{ 'index':i,'tag':E.QName(x).localname,'text':tx(x),'properties':[E.tostring(p).decode() for p in x.xpath('.//w:pPr|.//w:rPr|.//w:tblPr|.//w:tblGrid|.//w:tcPr|.//w:trPr',namespaces=N)]} for i,x in enumerate(tree.find('w:body',N))]}
(BASE/'reference-inventory.json').write_text(json.dumps(inventory,indent=2))
shutil.copyfile(REF,BASE/'working.docx')
unpacked=BASE/'clone-unpacked';patch.unzip_docx(BASE/'working.docx',unpacked)
tree=E.parse(str(unpacked/'word/document.xml'));body=tree.getroot().find('w:body',N);slots=list(body)
def replace(el,text):
    nodes=el.findall('.//w:t',N)
    assert nodes,(E.QName(el).localname,text)
    nodes[0].text=text
    for t in nodes[1:]:t.text=''
def p(i,text):replace(slots[i],text)
def table(i,rows,offset=0):
    trs=slots[i].findall('w:tr',N)
    assert len(trs)>=len(rows)+offset,(i,len(trs),len(rows))
    for tr,row in zip(trs[offset:],rows):
        cells=tr.findall('w:tc',N);assert len(cells)==len(row),(i,len(cells),len(row))
        for cell,text in zip(cells,row):replace(cell,text)

p(8,'ProjectOS M3 Software Foundation');p(9,'After Dark Phoenix First Project Plan')
# The longer approved names use two lines each. Consume cover spacers rather
# than shrinking the template type or splitting the metadata table.
cover_spacers={0,1,2,3,10,11,12,13}
for i in sorted(cover_spacers):body.remove(slots[i])
page_break=E.SubElement(slots[23].find('w:pPr',N),'{'+patch.W_NS+'}pageBreakBefore');page_break.set('{'+patch.W_NS+'}val','1')
for node in slots[20].findall('.//w:t',N):
    if node.text in {'[Draft / Proposed / Approved]','[Team Name]','[Month, DD, YYYY]'}:
        node.text={'[Draft / Proposed / Approved]':'Proposed','[Team Name]':'ProjectOS owner','[Month, DD, YYYY]':'October 15, 2026'}[node.text]
replace(slots[20].findall('.//w:tc',N)[2].findall('w:p',N)[1],'ProjectOS owner')
table(22,[['Authors','Codex'],['Reviewers','Owner'],['Related docs','M3 acceptance and checkpoint verification'],['Scope','Local repository inspection, isolated worktree review, export and first project.']])
p(24,'We propose a bounded Software foundation for ProjectOS and its first permanent project, After Dark Phoenix. The trusted native layer inspects a selected repository, creates an isolated worktree, binds a review to exact file bytes and exports a patch. Ordinary project files remain authoritative. The owner reviews the proposal before any future canonical application.')
p(25,'The first project researches unequal overnight urban heat in Phoenix for residents and civic practitioners. Treg discovery cost $0.092; an approved $0.09 Phoenix correction is capacity-blocked. A 1,200–1,600 word article is drafted only in staging. Canonical application, workers, user-repository tests and app control remain excluded. Gate C and owner acceptance govern expansion.')
table(28,[['Inspect the exact selected repository and HEAD.','Automatic parent-repository discovery or network Git.'],['Create staging without changing canonical content.','Canonical patch application or worker execution.'],['Persist redacted receipts and reject stale exports.','Repository test execution or application control.'],['Export the first sourced Phoenix article proposal.','Publication, automatic commits, pushes or PRs.']],1)
p(31,'ProjectOS already provides a local draft editor, a single writer session, graph metadata and explicit AI proposals. The unfinished M3 slice needed compile repairs, project-type authority, Software receipts and native verification. A build alone could not establish the repository boundary or prove that review and export preserve canonical files.')
p(32,'The UI chooses intent and displays native results. Tauri commands own project identity and approval checks; DocumentStore owns sidecar access; SoftwareBroker owns local Git and staging identities. Treg ingestion is a separate budgeted research step. The invariant is that a review authorizes only the bytes captured from one managed run at one base HEAD.')
p(36,'Figure 1. ProjectOS Software review and export architecture.')
table(40,[['ProjectOS UI','Choose type, inspect, review and export.','Transient renderer state','Display refusal; invalidate stale review.'],['Trusted commands','Bind project, native IDs and explicit approval.','Writer session and manifest','Reject mismatch or busy session.'],['SoftwareBroker and Git','Read bounded files; create inert staging.','Git objects and managed worktree','Deny unsafe config, links or stale bytes.'],['Workspace and graph','Persist outcomes and expose Activity.','Local .projectos metadata','Report partial effects; block blind retry.'],['Treg ingestion','Retrieve raw SERP and keyword evidence.','Raw JSON and ledger','Enforce budget and provider policy.']],1)
life=[
'Choose Substack or Software before registration. workspace_open(folder, create, projectType?) requires a type for new registrations; existing manifests remain authoritative.',
'Open only the selected repository root. Validate metadata, bounds and type before enabling Software. Status inspection performs no checkout or network request.',
'On explicit create, bind project UUID, run ID and base HEAD. Git creates a detached worktree without checkout; the broker writes inert object bytes into staging.',
'Populate the managed worktree with the article and outline update. Review bounded UTF-8 bytes, modes and diffs; store native review ID and content fingerprint.',
'Persist create, review, refusal and export outcomes through Workspace. If an effect succeeds but its receipt fails, report that partial result and block blind retry.',
'Export revalidates HEAD and all touched bytes before and after the destination dialog. Use create-new output semantics. Git calls have a 15-second deadline and 16 MiB output bound.',
'Save the export receipt, expose it in Activity, compare review/export fingerprints and verify the canonical tracked tree. Quit releases the writer lock; owner acceptance remains separate.'
]
for i,text in enumerate(life,44):p(i,text)
table(55,[['id and at','String and u64','Yes','Unique receipt ID and UTC epoch seconds.'],['operation','String','Yes','CreateWorktree, ReviewPatch or ExportPatch.'],['project_uuid and run_id','Strings','Yes','Manifest UUID and managed staging run identity.'],['base_head','Object ID','Yes','Exact commit from which staging starts.'],['review_hash','Optional string','For review/export','SHA256 over paths, modes, before/after bytes and base HEAD.'],['changed_paths and capability_scope','String arrays','Yes','Relative paths and bounded granted operation scope.'],['outcome and failure','String and optional string','Yes / No','Outcome plus static redacted failure; never absolute machine paths.']],1)
for i,text in {59:'workspace_open, software_status, software_create_worktree, software_review and software_export_review are the only foundation interfaces. No software_apply command exists.',60:'Renderer arguments use projectType, worktreeId, reviewId and contentHash. Native state owns the corresponding records; renderer paths cannot select arbitrary staging.',61:'WorkspaceView exposes declared_type and software_receipts. Persist receipts in software-receipts.toml and graph records; never persist staging roots or export destinations.',62:'Receipt evidence describes observed file review and export. It does not establish test execution, a resulting commit or a canonical mutation.',63:'The source contracts are maintained with the implementation in',64:'app/src-tauri/src/workspace.rs and software_commands.rs; update them with each contract change.'}.items():p(i,text)
p(68,'The mutex serializes operations. Reviews bind base HEAD, paths, modes and before/after bytes; changes require fresh review. Export uses create-new semantics. Treg selection matches inputs, then reliability sample, price and last OK. Keys are SHA256(endpoint ID plus canonical parameters), reused only for unknown-result retries. A known capacity refusal is not an unknown result.')
table(69,[['Repeated create or export','Require explicit action; preserve existing exports.','No automatic side-effect retry.'],['Receipt persistence fails','Report whether staging or export already exists; block continuation.','An unreceipted effect cannot be reported as a clean failure.'],['Treg 429, 5xx or timeout','Switch provider within the approved ceiling; no unchanged 4xx retry.','Reliability and bounded spending take precedence.'],['HEAD or staged bytes change','Refuse export and require fresh review.','The captured content remains authoritative.']],1)
security=[
'Bind each Software operation to the active manifest UUID. A publication project cannot use Software. The renderer supplies intent and native-owned IDs, never Git arguments.',
'Receipts contain relative changed paths and static failure descriptions. Exclude credentials, content payloads, staging roots, export destinations and absolute machine paths.',
'Treg authentication uses its login flow, without provider API keys. Existing OpenRouter credentials stay in the OS keyring; no production inference is required for this sprint.',
'Discard ambient GIT variables. Disable hooks, fsmonitor, credential helpers, external diff, textconv and filters. Reject unsafe config, nested repositories, symlinks, binary and oversized changes.',
'Keep raw research responses and claim provenance locally. GitHub README blobs are implementation examples. Institutional sources support article facts. Retain dirty staging for owner review; do not remove user checkouts.'
]
for i,text in enumerate(security,73):p(i,text)
table(81,[['Foundation checks','All locked suites and native debug build pass.','Codex','Required'],['Bounded local processing','15-second Git deadline; 1,000 files / 16 MiB.','Codex','Required'],['Research spending','Settled $0.092; approved total ceiling $0.182.','Owner','Blocked correction'],['Source drift','Hash mismatch blocks roadmap verification.','Codex','Required'],['Export integrity','Review/export fingerprints agree; canonical tree clean.','Owner','Required']],1)
rollout=slots[81].findall('w:tr',N)[-1].findall('w:tc',N)
# The source rollout constraint is a merged final row, retained in place.
if len(rollout)==1:replace(rollout[0],'Rollout constraint: foundation only. Signing/notarization resources and Linux evidence block Gate C. Live provider/keyring conformance needs owner opt in. Full M3 and release require the owner’s final acceptance.')
table(86,[['Direct canonical application','Immediate edits in the main checkout.','Deferred until graph-owned multi-file recovery.'],['Git checkout for staging','Simple standard worktree workflow.','Could invoke repository filters; populate inert bytes instead.'],['Generic external search fallback','Convenient research substitution.','Treg population is mandatory and cost bounded.'],['Automatic workers and desktop control','A complete Software golden path.','Gate C confinement and consent evidence are incomplete.']],1)
for i,text in {89:'No unresolved design questions remain for this foundation sprint. The approved boundary and interfaces are fixed.',90:'External blockers: Linux and signing resources gate Gate C. Treg capacity blocks the approved Phoenix correction; national volume is not Phoenix evidence.',91:'External prerequisite: owner opt in is required for live OpenRouter and OS-keyring conformance; neither is claimed here.',92:'Acceptance gate: the owner reviews the patch and workflow. m3.ui, m3.effects and m3.e2e remain planned or blocked.',95:'Adopt the foundation as a local checkpoint. Complete automated and native checks, retain source-bound evidence, export the first article proposal, and publish the ProjectOS checkpoint only under owner authority. Preserve a month-long handoff with exact resume commands and blockers. Do not promote the full M3 milestone.'}.items():p(i,text)
table(96,[['Foundation','Typed registration, broker and receipts.','Locked Rust, Node, syntax and debug checks pass.'],['First project','Permanent Software project and bootstrap.','Treg evidence retained; canonical files match bootstrap.'],['Native verification','Two-repository isolation and review/export.','Stale refusal, keyboard focus and lock release inspected.'],['Checkpoint and pause','Design, evidence, roadmap and handoff.','Publish progress; retain Gate C and owner gates.']],1)
for el in tree.findall('.//wp:docPr',N):el.set('descr','ProjectOS UI passes intent to trusted commands and SoftwareBroker; Git creates staging, Workspace stores graph receipts, Treg supplies research, and a reviewed patch is exported.')
tree.write(str(unpacked/'word/document.xml'),xml_declaration=True,encoding='UTF-8',standalone=True)
footer=E.parse(str(unpacked/'word/footer1.xml'))
for el in footer.findall('.//w:t',N):
    if el.text:el.text=el.text.replace('[Organization Name]','ProjectOS')
footer.write(str(unpacked/'word/footer1.xml'),xml_declaration=True,encoding='UTF-8',standalone=True)
foot=E.parse(str(unpacked/'word/footnotes.xml'))
for el in foot.findall('.//w:t',N):
    if el.text and 'Use footnotes' in el.text:el.text=' Foundation verification establishes local review and export only. Gate C and owner acceptance remain open.'
foot.write(str(unpacked/'word/footnotes.xml'),xml_declaration=True,encoding='UTF-8',standalone=True)
shutil.copyfile(BASE/'architecture.png',unpacked/'word/media/image1.png')
rels_path=unpacked/'word/_rels/document.xml.rels'
rels=E.parse(str(rels_path))
for relationship in rels.getroot():
    if relationship.get('Id')=='rId9':relationship.set('Target','https://github.com/Bonorinoa/projectos/blob/main/app/src-tauri/src/software_commands.rs')
rels.write(str(rels_path),xml_declaration=True,encoding='UTF-8',standalone=True)
patch.zip_docx(unpacked,OUT)
allowed={'word/document.xml','word/media/image1.png','word/footer1.xml','word/footnotes.xml','word/_rels/document.xml.rels'}
with ZipFile(REF) as a,ZipFile(OUT) as b:
    assert set(a.namelist())==set(b.namelist())
    changed=[n for n in a.namelist() if a.read(n)!=b.read(n)]
    assert set(changed)<=allowed,changed
    ar=E.fromstring(a.read('word/document.xml'));br=E.fromstring(b.read('word/document.xml'))
    ab=ar.find('w:body',N);original_slots=list(ab)
    for i in sorted(cover_spacers):ab.remove(original_slots[i])
    comparison=br.find('w:body',N)[23-len(cover_spacers)].find('w:pPr/w:pageBreakBefore',N)
    comparison.getparent().remove(comparison)
    for xpath in ['.//w:sectPr','.//w:pPr','.//w:rPr','.//w:tblPr','.//w:tblGrid','.//w:tcPr','.//w:trPr']:
        aa=[E.tostring(x) for x in ar.findall(xpath,N)];bb=[E.tostring(x) for x in br.findall(xpath,N)];assert aa==bb,xpath
    assert not any('[' in t for t in br.xpath('.//w:t/text()',namespaces=N)), 'Unfilled placeholder'
    result={'referenceSha256':hashlib.sha256(REF.read_bytes()).hexdigest(),'finalSha256':hashlib.sha256(OUT.read_bytes()).hexdigest(),'changedParts':changed,'preservedParts':[n for n in a.namelist() if n not in changed],'sectionAndStyleAndTablePropertiesUnchangedExceptDeclaredCoverFlow':True,'relationshipException':'Only rId9 hyperlink target updated from example.com placeholder to the public source contract; IDs and all other relationship attributes preserved.','fields':inventory['structure']['fields']}
(BASE/'package-qa.json').write_text(json.dumps(result,indent=2)+'\n')
print(json.dumps(result,indent=2))
