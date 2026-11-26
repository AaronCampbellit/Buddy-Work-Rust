use serde::{Deserialize, Serialize};
use std::{collections::BTreeMap, fs, io::{Read, Write}, path::Path, sync::atomic::{AtomicU64, Ordering}};
use windows::core::{PCWSTR, HSTRING};
use windows::Win32::Foundation::{LocalFree,HLOCAL};
use windows::Win32::Security::Cryptography::*;
use windows::Win32::Storage::FileSystem::{MoveFileExW,MOVEFILE_REPLACE_EXISTING,MOVEFILE_WRITE_THROUGH};

type Rows = Vec<Vec<String>>;
type Collections = BTreeMap<String, Rows>;

// The revision lives only in memory. All mutable access to a collection that
// contributes to a pinned note is tracked, including edits followed by rollback.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(transparent)]
pub struct Records {
    collections: Collections,
    #[serde(skip, default = "next_revision")]
    preview_revision: u64,
}

fn next_revision() -> u64 {
    static REVISION: AtomicU64 = AtomicU64::new(1);
    REVISION.fetch_add(1, Ordering::Relaxed)
}

impl Default for Records {
    fn default() -> Self {
        Self { collections: Collections::new(), preview_revision: next_revision() }
    }
}

impl PartialEq for Records {
    fn eq(&self, other: &Self) -> bool { self.collections == other.collections }
}

impl std::ops::Deref for Records {
    type Target = Collections;
    fn deref(&self) -> &Collections { &self.collections }
}

impl Records {
    pub(crate) fn new() -> Self { Self::default() }
    pub(crate) fn preview_revision(&self) -> u64 { self.preview_revision }

    fn will_mutate(&mut self, key: &str) {
        if matches!(key, "notes" | "settings") || key.starts_with("note-format/") || key.starts_with("note-image-positions/") {
            self.preview_revision = next_revision();
        }
    }
    pub(crate) fn entry(&mut self, key: String) -> std::collections::btree_map::Entry<'_, String, Rows> {
        self.will_mutate(&key);
        self.collections.entry(key)
    }
    pub(crate) fn get_mut(&mut self, key: &str) -> Option<&mut Rows> {
        self.will_mutate(key);
        self.collections.get_mut(key)
    }
    pub(crate) fn remove(&mut self, key: &str) -> Option<Rows> {
        self.will_mutate(key);
        self.collections.remove(key)
    }
    pub(crate) fn insert(&mut self, key: String, rows: Rows) -> Option<Rows> {
        self.will_mutate(&key);
        self.collections.insert(key, rows)
    }
    pub(crate) fn extend(&mut self, collections: Collections) {
        for (key, rows) in collections { self.insert(key, rows); }
    }
}
const STATE_MAX: usize = 32 * 1024 * 1024;
static SEQUENCE: AtomicU64 = AtomicU64::new(0);
#[derive(Serialize,Deserialize)]
struct Store {version:u32,records:Records}

fn protect(input:&[u8], encrypt:bool)->Result<Vec<u8>,String>{
    let input=CRYPT_INTEGER_BLOB{ cbData: u32::try_from(input.len()).map_err(|_| "Protected content is too large.")?, pbData:input.as_ptr().cast_mut() };
    let mut output=CRYPT_INTEGER_BLOB::default();
    let result=unsafe { if encrypt {
        CryptProtectData(&input, PCWSTR::null(), None, None, None, CRYPTPROTECT_UI_FORBIDDEN, &mut output)
    } else {
        CryptUnprotectData(&input, None, None, None, None, CRYPTPROTECT_UI_FORBIDDEN, &mut output)
    }};
    result.map_err(|e|format!("Windows user encryption is unavailable ({:#010x}).",e.code().0))?;
    let bytes=unsafe { if output.cbData==0{Vec::new()}else{std::slice::from_raw_parts(output.pbData,output.cbData as usize).to_vec()} };
    if !output.pbData.is_null(){unsafe { LocalFree(Some(HLOCAL(output.pbData.cast()))); }}
    Ok(bytes)
}

pub fn write_protected(path:&Path,bytes:&[u8])->Result<(),String>{
    let encrypted=protect(bytes,true)?;
    let dir=path.parent().ok_or("Invalid storage location.")?;
    fs::create_dir_all(dir).map_err(|e|e.to_string())?;
    let tmp=dir.join(format!(".buddy-{}-{}.tmp",std::process::id(),SEQUENCE.fetch_add(1,Ordering::Relaxed)));
    let mut out=fs::OpenOptions::new().write(true).create_new(true).open(&tmp).map_err(|e|e.to_string())?;
    let result=(||{
        out.write_all(&encrypted).and_then(|_|out.sync_all()).map_err(|e|e.to_string())?;
        drop(out);
        unsafe{MoveFileExW(&HSTRING::from(tmp.as_os_str()),&HSTRING::from(path.as_os_str()),MOVEFILE_REPLACE_EXISTING|MOVEFILE_WRITE_THROUGH)}.map_err(|e|e.to_string())
    })();
    if result.is_err(){let _=fs::remove_file(tmp);}
    result
}
pub fn read_protected(path:&Path,max:usize)->Result<Vec<u8>,String>{
    let mut bytes=Vec::new();
    let bound=max.saturating_add(65536);
    fs::File::open(path).map_err(|e|e.to_string())?.take((bound as u64).saturating_add(1)).read_to_end(&mut bytes).map_err(|e|e.to_string())?;
    if bytes.len()>bound{return Err("Protected data exceeds the allowed size.".into())}
    let plain=protect(&bytes,false)?;
    if plain.len()>max{return Err("Unprotected data exceeds the allowed size.".into())}
    Ok(plain)
}
pub fn load(profile:&Path)->Result<Records,String>{
    let path=profile.join("state-v1.dpapi");
    match fs::metadata(&path){Err(e) if e.kind()==std::io::ErrorKind::NotFound=>return Ok(Records::new()),Err(e)=>return Err(e.to_string()),_=>{}}
    let data=read_protected(&path,STATE_MAX)?;
    let state:Store=serde_json::from_slice(&data).map_err(|_|"The saved state could not be decoded.")?;
    if state.version!=1 {return Err("This profile requires a newer version of Buddy.".into())}
    Ok(state.records)
}
pub fn save(profile:&Path,records:&Records)->Result<(),String>{
    #[derive(Serialize)] struct Snapshot<'a>{version:u32,records:&'a Records}
    let bytes=serde_json::to_vec(&Snapshot{version:1,records}).map_err(|e|e.to_string())?;
    if bytes.len()>STATE_MAX{return Err("Buddy's protected state is full.".into())}
    write_protected(&profile.join("state-v1.dpapi"),&bytes)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn note_revision_tracks_preview_inputs_and_preserves_the_wire_format() {
        let mut state = Records::new();
        let notes: Rows = (0..1000).map(|i|vec![i.to_string(), "Title".into()]).collect();
        state.insert("notes".into(), notes);
        let first = state.preview_revision();
        for _ in 0..300 {
            assert_eq!(state["notes"].len(), 1000);
            state.entry("downloads".into()).or_default();
            assert_eq!(state.preview_revision(), first);
        }
        for key in ["settings", "note-format/0", "note-image-positions/0", "notes"] {
            let before = state.preview_revision();
            state.entry(key.into()).or_default().push(vec!["edit".into()]);
            assert_ne!(state.preview_revision(), before);
        }
        let previous = state.clone();
        state.get_mut("notes").unwrap()[0][1].push('!');
        assert_ne!(state.preview_revision(), previous.preview_revision());
        let disk = serde_json::to_vec(&previous).unwrap();
        assert_eq!(disk, serde_json::to_vec(&previous.collections).unwrap());
        let loaded: Records = serde_json::from_slice(&disk).unwrap();
        assert_eq!(loaded, previous);
        assert_ne!(loaded.preview_revision(), previous.preview_revision());
    }
}
