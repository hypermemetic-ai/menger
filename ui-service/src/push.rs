//! UI-3 / UI-7: **a push** — the signer checked, then (UI-7) every
//! component's source compiled in the sealed compiler ([`Compiler`]) and
//! compared with the tree the bundle claims (or the tree filled in when the
//! bundle holds none), every `manifest.components.<name>.uses` checked
//! against the running tickets service through the [`Seam`], and the
//! manifest's `contract` compared with that service's hash — all before the
//! store's one transaction (UI-3's) opens.

use crate::compiler::{CompileFailed, Compiled, Compiler};
use crate::events::{Reason, UiEvent};
use crate::seam::{Schemas, Seam};
use crate::signers::Signers;
use crate::store::{digest_of, hash_of, ManifestDoc, NewVersion, UiError, UiStore};
use crate::vocab::{Bundle, ComponentName, Manifest, Signature};
use std::collections::BTreeMap;
use std::io::Read;
use menger_tickets_api::vocab::CommitRef;
use std::sync::Arc;

/// UI-11: the contract a manifest names when it was built against no
/// running service (`scripts/ui-build.sh` without `--contract-from`): the
/// match is unknown, never `false`.
const UNKNOWN: &str = "unknown";

/// What a push runs before the store: the compiler (`--ui-compiler`; none:
/// the trees are stored as given, `compiled: false`) and the seam to the
/// running tickets service.
#[derive(Clone, Debug, Default)]
pub struct PushSteps {
    pub compiler: Option<Arc<Compiler>>,
    pub seam: Seam,
}

/// The push as the activation runs it: who pushed, what, signed how.
pub(crate) struct Push {
    pub by: String,
    pub bundle: Bundle,
    pub manifest: Manifest,
    pub signature: Signature,
    pub commit: Option<CommitRef>,
}

/// The store's refusal, in band: the caller's mistakes `refused`, the
/// file's own failures `error`.
pub(crate) fn refusal(e: UiError) -> UiEvent {
    let reason = match &e {
        UiError::BadBundle(_) => Reason::BadBundle,
        UiError::BadManifest(_) => Reason::BadManifest,
        UiError::UnknownVersion(_) => Reason::UnknownVersion,
        UiError::UnknownComponent { .. } => Reason::UnknownComponent,
        UiError::UnknownSlot { .. } => Reason::UnknownSlot,
        UiError::Db(_) => return UiEvent::Error { message: e.to_string() },
    };
    UiEvent::refused(reason, e.to_string())
}

fn now() -> i64 {
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0, |d| d.as_secs() as i64)
}

/// `push`: a signer named, the signature over the upload's digest by one
/// of them, UI-7's steps ([`checked`]), then the store's one transaction.
pub(crate) async fn push(store: Arc<UiStore>, signers: Arc<Signers>, steps: PushSteps, p: Push) -> UiEvent {
    if signers.is_empty() {
        let message = "no --ui-signer is configured on this server: every push is refused";
        return UiEvent::refused(Reason::NoSigner, message);
    }
    let upload = p.bundle.bytes();
    if !signers.verify(&digest_of(&upload), &p.signature.bytes()) {
        let message = "the signature does not verify: Ed25519 over the bundle's sha256 by a --ui-signer";
        return UiEvent::refused(Reason::BadSignature, message);
    }
    let manifest = p.manifest.into_string();
    let uploaded = hash_of(&upload);
    let checked = match checked(&steps, upload, &manifest).await {
        Ok(c) => c,
        Err(refused) => return refused,
    };
    let v = NewVersion { bundle: checked.bundle, manifest, commit: p.commit.map(|c| c.to_string()), pushed_by: p.by, pushed_at: now() };
    let stored = tokio::task::spawn_blocking(move || store.push_uploaded(v, &uploaded)).await;
    match stored {
        Ok(Ok(s)) => UiEvent::Pushed {
            version: s.version,
            uploaded: s.uploaded,
            live: s.live,
            new: s.new,
            contract_matches: checked.contract_matches,
            compiled: checked.compiled,
        },
        Ok(Err(e)) => refusal(e),
        Err(e) => UiEvent::Error { message: format!("the ui store failed: {e}") },
    }
}

/// What the steps answer: the bytes to store (the upload, or the upload
/// with its trees filled in), whether the server compiled them, and whether
/// the manifest's contract is the running tickets service's hash (`None`:
/// the manifest says `unknown`).
struct Checked {
    bundle: Vec<u8>,
    compiled: bool,
    contract_matches: Option<bool>,
}

/// A refusal naming a component.
fn naming(reason: Reason, name: &str, message: String) -> UiEvent {
    UiEvent::Refused { reason, message, name: Some(name.to_string()), line: None, method: None }
}

/// The steps before the transaction: every `src/<name>.svelte` compiled in
/// the sandbox and compared with the bundle's tree and handlers (filled in
/// when absent); both manifests checked against what compiled; every `uses`
/// looked up in the running tickets service through the seam, as the
/// system; the contract compared with that service's hash. Without a
/// compiler the trees are taken as given and only the lint and the
/// contract run.
async fn checked(steps: &PushSteps, upload: Vec<u8>, manifest: &str) -> Result<Checked, UiEvent> {
    let param = ManifestDoc::parse(manifest).map_err(refusal)?;
    let files = Files::read(&upload).map_err(refusal)?;
    let compiled = match &steps.compiler {
        None => None,
        Some(compiler) => Some(compile_all(compiler.clone(), &files).await?),
    };
    let bundle = match &compiled {
        None => upload,
        Some(trees) => {
            agree(&param, &files, trees)?;
            fill(&upload, &files, trees).map_err(refusal)?
        }
    };
    let Some(transport) = steps.seam.system() else {
        return Err(UiEvent::Error { message: "the ui service is not attached to the running service: nothing to lint against".into() });
    };
    let mut schemas = Schemas::new(transport);
    for (name, entry) in &param.components {
        for method in uses_of(entry) {
            match schemas.exposes(&method).await {
                Ok(true) => {}
                Ok(false) => {
                    let message = format!("{name} uses {method}, which the running tickets service does not expose");
                    return Err(UiEvent::Refused { reason: Reason::UnknownMethod, message, name: Some(name.clone()), line: None, method: Some(method) });
                }
                Err(message) => return Err(UiEvent::Error { message }),
            }
        }
    }
    let hash = schemas.hash().await.map_err(|message| UiEvent::Error { message })?;
    let contract_matches = match param.contract.as_deref().map(str::trim) {
        Some(UNKNOWN) => None,
        contract => Some(contract == Some(hash.as_str())),
    };
    Ok(Checked { bundle, compiled: compiled.is_some(), contract_matches })
}

/// A manifest entry's `uses`, each a string.
fn uses_of(entry: &serde_json::Value) -> Vec<String> {
    entry["uses"].as_array().map(|a| a.iter().filter_map(|u| u.as_str().map(str::to_string)).collect()).unwrap_or_default()
}

/// The files a push reads of a bundle: the sources, the compiled trees and
/// handlers, and `manifest.json`.
struct Files {
    sources: BTreeMap<String, String>,
    trees: BTreeMap<String, Vec<u8>>,
    handlers: BTreeMap<String, Vec<u8>>,
    manifest: Option<Vec<u8>>,
}

impl Files {
    fn read(bundle: &[u8]) -> Result<Self, UiError> {
        let mut f = Files { sources: BTreeMap::new(), trees: BTreeMap::new(), handlers: BTreeMap::new(), manifest: None };
        let bad = |e: std::io::Error| UiError::BadBundle(e.to_string());
        let mut archive = tar::Archive::new(bundle);
        for entry in archive.entries().map_err(bad)? {
            let mut entry = entry.map_err(bad)?;
            if !entry.header().entry_type().is_file() {
                continue;
            }
            let path = entry.path().map_err(bad)?.to_string_lossy().trim_start_matches("./").to_string();
            let mut body = Vec::new();
            entry.read_to_end(&mut body).map_err(bad)?;
            let stem = |dir: &str, ext: &str| path.strip_prefix(dir).and_then(|p| p.strip_suffix(ext)).filter(|n| !n.contains('/')).map(str::to_string);
            if let Some(name) = stem("src/", ".svelte") {
                let name = ComponentName::try_from(name).map_err(|e| UiError::BadBundle(format!("src/: {e}")))?.into_string();
                let src = String::from_utf8(body).map_err(|_| UiError::BadBundle(format!("src/{name}.svelte is not UTF-8")))?;
                f.sources.insert(name, src);
            } else if let Some(name) = stem("components/", ".json") {
                f.trees.insert(name, body);
            } else if let Some(name) = stem("components/", ".rhai") {
                f.handlers.insert(name, body);
            } else if path == "manifest.json" {
                f.manifest = Some(body);
            }
        }
        Ok(f)
    }
}

/// Every source compiled in one sandbox (one store for the push), off the
/// async threads; the first refusal, or a tree and handlers that differ
/// from the bundle's, refuses the push.
async fn compile_all(compiler: Arc<Compiler>, files: &Files) -> Result<BTreeMap<String, Compiled>, UiEvent> {
    let sources = files.sources.clone();
    let run = tokio::task::spawn_blocking(move || -> Result<BTreeMap<String, Compiled>, CompileFailed> {
        let mut out = BTreeMap::new();
        let Some(first) = sources.keys().next() else { return Ok(out) };
        let mut session = compiler.session(first)?;
        for (name, src) in &sources {
            out.insert(name.clone(), session.compile(name, src)?);
        }
        Ok(out)
    });
    let compiled = match run.await {
        Ok(Ok(c)) => c,
        Ok(Err(f)) => {
            let message = format!("src/{}.svelte does not compile (line {}): {}", f.name, f.line, f.message);
            return Err(UiEvent::Refused { reason: Reason::CompileFailed, message, name: Some(f.name), line: Some(f.line), method: None });
        }
        Err(e) => return Err(UiEvent::Error { message: format!("the compiler failed: {e}") }),
    };
    for (name, c) in &compiled {
        let differs = |given: Option<&Vec<u8>>, made: &str| given.is_some_and(|g| g.as_slice() != made.as_bytes());
        if differs(files.trees.get(name), &c.tree) {
            return Err(naming(Reason::CompileMismatch, name, format!("components/{name}.json is not the tree src/{name}.svelte compiles to")));
        }
        if differs(files.handlers.get(name), &c.handlers) {
            return Err(naming(Reason::CompileMismatch, name, format!("components/{name}.rhai is not the handlers src/{name}.svelte compiles to")));
        }
    }
    Ok(compiled)
}

/// The compiled tree's `uses` and the two files' hashes are the truth: the
/// push's `manifest` and the bundle's `manifest.json` must say the same for
/// every compiled component, or the push is refused `manifest_mismatch`.
fn agree(param: &ManifestDoc, files: &Files, compiled: &BTreeMap<String, Compiled>) -> Result<(), UiEvent> {
    let bundled: Option<serde_json::Value> = files.manifest.as_ref().and_then(|m| serde_json::from_slice(m).ok());
    for (name, c) in compiled {
        let tree: serde_json::Value = serde_json::from_str(&c.tree).unwrap_or_default();
        let uses = uses_of(&tree);
        let (tree_hash, handlers_hash) = (hash_of(c.tree.as_bytes()), hash_of(c.handlers.as_bytes()));
        let says = |entry: &serde_json::Value| -> Option<String> {
            if entry.is_null() {
                return Some("names no such component".into());
            }
            if uses_of(entry) != uses || !entry["uses"].is_array() {
                return Some(format!("says uses {}, the compiled tree {:?}", entry["uses"], uses));
            }
            if entry["tree"].as_str() != Some(tree_hash.as_str()) {
                return Some(format!("says tree {}, the compiled tree's sha256 is {tree_hash}", entry["tree"]));
            }
            if entry["handlers"].as_str() != Some(handlers_hash.as_str()) {
                return Some(format!("says handlers {}, the compiled handlers' sha256 is {handlers_hash}", entry["handlers"]));
            }
            None
        };
        let in_param = param.components.get(name).cloned().unwrap_or_default();
        if let Some(why) = says(&in_param) {
            return Err(naming(Reason::ManifestMismatch, name, format!("the push's manifest, for {name}, {why}")));
        }
        let in_bundle = bundled.as_ref().map(|b| b["components"][name.as_str()].clone()).unwrap_or_default();
        if let Some(why) = says(&in_bundle) {
            return Err(naming(Reason::ManifestMismatch, name, format!("the bundle's manifest.json, for {name}, {why}")));
        }
    }
    Ok(())
}

/// The upload with every compiled tree and handlers it lacks appended
/// (`components/<name>.json`, `.rhai`); the upload itself when it lacks
/// none — stored exactly as signed.
fn fill(upload: &[u8], files: &Files, compiled: &BTreeMap<String, Compiled>) -> Result<Vec<u8>, UiError> {
    let mut missing: Vec<(String, &str)> = Vec::new();
    for (name, c) in compiled {
        if !files.trees.contains_key(name) {
            missing.push((format!("components/{name}.json"), &c.tree));
        }
        if !files.handlers.contains_key(name) {
            missing.push((format!("components/{name}.rhai"), &c.handlers));
        }
    }
    if missing.is_empty() {
        return Ok(upload.to_vec());
    }
    let bad = |e: std::io::Error| UiError::BadBundle(e.to_string());
    let mut out = tar::Builder::new(Vec::new());
    let mut archive = tar::Archive::new(upload);
    for entry in archive.entries().map_err(bad)? {
        let mut entry = entry.map_err(bad)?;
        let header = entry.header().clone();
        let mut body = Vec::new();
        entry.read_to_end(&mut body).map_err(bad)?;
        out.append(&header, body.as_slice()).map_err(bad)?;
    }
    for (path, body) in missing {
        let mut h = tar::Header::new_gnu();
        h.set_size(body.len() as u64);
        h.set_mode(0o644);
        h.set_mtime(0);
        h.set_cksum();
        out.append_data(&mut h, path, body.as_bytes()).map_err(bad)?;
    }
    out.into_inner().map_err(bad)
}
