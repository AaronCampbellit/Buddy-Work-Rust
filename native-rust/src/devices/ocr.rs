use image::{DynamicImage, ImageReader};
use ocrs::{ImageSource, OcrEngine, OcrEngineParams, TextItem};
use rten::Model;
use std::io::Cursor;
use std::path::Path;
use std::sync::{Mutex, OnceLock, atomic::{AtomicBool, Ordering}};
use std::time::{Duration, Instant};
use sha2::{Digest,Sha256};

#[derive(Clone, Debug)]
pub struct Word {
    pub text: String,
    pub x: i32,
    pub y: i32,
    pub width: i32,
    pub height: i32,
}
#[derive(Clone,Debug)]
pub struct Scan { pub text: String, pub lines: Vec<Vec<Word>> }

pub(super) fn decode(bytes: &[u8], max_pixels:u64) -> Result<DynamicImage,String> {
    if bytes.is_empty() || bytes.len() > 25 * 1024 * 1024 { return Err("Choose an image under 25 MB.".into()); }
    let reader = || ImageReader::new(Cursor::new(bytes)).with_guessed_format().map_err(|e|e.to_string());
    let (w,h) = reader()?.into_dimensions().map_err(|e|e.to_string())?;
    if w == 0 || h == 0 || u64::from(w)*u64::from(h) > max_pixels {return Err("The image exceeds the scanner's dimensions.".into());}
    reader()?.decode().map_err(|e| e.to_string())
}
fn model(folder:&Path, name:&str, expected:&str) -> Result<Model,String> {
    let bytes = std::fs::read(folder.join(name)).map_err(|_|format!("Reinstall the local OCR model {name}."))?;
    if format!("{:x}", Sha256::digest(&bytes)) != expected { return Err(format!("OCR model verification failed for {name}.")); }
    Model::load(bytes).map_err(|e|e.to_string())
}
fn open_engine(folder:&Path) -> Result<OcrEngine,String> {
    OcrEngine::new(OcrEngineParams {
        detection_model:Some(model(folder,"text-detection.onnx","a917b23dbd9524b465df7e922641b3ff2981623df4ded5a0234004ef2fee7cfe")?),
        recognition_model:Some(model(folder,"text-recognition.onnx","86c145c2edb96c8caed5b1ebb8f44d706408922211451c309c625157dd6061c5")?),
        ..Default::default()
    }).map_err(|e|e.to_string())
}
struct Cache {
    engine: Option<OcrEngine>,
    last_use: Instant,
}
static ENGINE: OnceLock<Mutex<Cache>> = OnceLock::new();
static RELEASE: AtomicBool = AtomicBool::new(false);

pub fn release(force: bool) {
    let Some(slot) = ENGINE.get() else { return };
    if force { RELEASE.store(true, Ordering::Release); }
    // Housekeeping must never wait for inference.
    if let Ok(mut cache) = slot.try_lock() {
        if RELEASE.swap(false, Ordering::AcqRel) || cache.last_use.elapsed() >= Duration::from_secs(60) {
            cache.engine.take();
        }
    }
}
pub fn scan(bytes:&[u8]) -> Result<Scan,String> {
    let folder = std::env::current_exe().map_err(|e|e.to_string())?.parent().ok_or("Missing application folder")?.join("models");
    scan_in(bytes,&folder)
}
pub fn scan_in(bytes:&[u8],folder:&Path)->Result<Scan,String>{
    // Each caller runs on a worker thread. Queue inference before allocating pixel
    // buffers so multiple editor windows cannot duplicate the model working set.
    let cache=ENGINE.get_or_init(|| Mutex::new(Cache {engine:None, last_use:Instant::now()}));
    let mut lock=cache.lock().map_err(|_|"The image scanner is unavailable.")?;
    let result=recognize(&mut lock,bytes,folder);
    lock.last_use=Instant::now();
    drop(lock);
    // A pause or session lock may have requested a release during recognition.
    if RELEASE.load(Ordering::Acquire) { release(false); }
    result
}
fn recognize(cache:&mut Cache, bytes:&[u8],folder:&Path)->Result<Scan,String>{
    let full=decode(bytes,16_000_000)?;
    let (w,h)=(full.width(),full.height());
    let scale=1_f64.min(3200.0 / w.max(h) as f64).min((4_000_000.0/(w as f64*h as f64)).sqrt());
    let image=if scale<1. {full.resize((scale*w as f64).round().max(1.) as u32,(scale*h as f64).round().max(1.) as u32,image::imageops::FilterType::Triangle)}else{full}.into_rgb8();
    let src=ImageSource::from_bytes(image.as_raw(),image.dimensions()).map_err(|e|e.to_string())?;
    if cache.engine.is_none(){cache.engine=Some(open_engine(folder)?);}
    let engine=cache.engine.as_ref().unwrap();
    let input=engine.prepare_input(src).map_err(|e|e.to_string())?;
    let detected=engine.detect_words(&input).map_err(|e|e.to_string())?;
    let boxes=engine.find_text_lines(&input,&detected);
    let read=engine.recognize_text(&input,&boxes).map_err(|e|e.to_string())?;
    let mut lines=Vec::new();let mut strings=Vec::new();let mut length=0;
    for line in read.into_iter().flatten(){
        let mut words=Vec::new();
        for word in line.words(){
            let text=word.to_string(); length+=text.len()+1;
            if length>20_000{return Err("Scan a smaller region (20,000 character limit).".into());}
            let rect=word.bounding_rect();
            let x=((rect.left() as f64 / scale).floor() as i32).clamp(0,w as i32);
            let y=((rect.top() as f64 / scale).floor() as i32).clamp(0,h as i32);
            let right=((rect.right() as f64 / scale).ceil() as i32).clamp(x,w as i32);
            let bottom=((rect.bottom() as f64 / scale).ceil() as i32).clamp(y,h as i32);
            if !text.is_empty() && right>x && bottom>y {words.push(Word{text,x,y,width:right-x,height:bottom-y});}
        }
        if !words.is_empty(){strings.push(words.iter().map(|v|v.text.as_str()).collect::<Vec<_>>().join(" "));lines.push(words);}
    }
    Ok(Scan{text:strings.join("\r\n"),lines})
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rejects_oversize_dimensions_before_decoding() {
        let data = include_bytes!("../../tests/fixtures/buddy-ocr.png");
        assert!(decode(data, 799 * 240).is_err());
        assert_eq!(decode(data, 800 * 240).unwrap().width(), 800);
    }

    #[test]
    #[ignore = "set BUDDY_TEST_OCR_MODELS to the staged, pinned models directory"]
    fn reads_two_lines_offline() {
        let folder = std::env::var_os("BUDDY_TEST_OCR_MODELS").expect("models directory");
        let result = scan_in(include_bytes!("../../tests/fixtures/buddy-ocr.png"), Path::new(&folder)).unwrap();
        assert_eq!(result.text, "Buddy OCR 12345\r\nSelect individual characters");
        assert_eq!(result.lines.len(), 2);
        assert_eq!(result.lines.iter().flatten().count(), 6);
        for word in result.lines.iter().flatten() {
            assert!(word.x >= 0 && word.y >= 0 && word.width > 0 && word.height > 0);
            assert!(word.x + word.width <= 800 && word.y + word.height <= 240);
        }
        let cache=ENGINE.get().unwrap();
        {
            let mut guard=cache.lock().unwrap();
            assert!(guard.engine.is_some());
            guard.last_use=Instant::now()-Duration::from_secs(61);
        }
        release(false);
        assert!(cache.lock().unwrap().engine.is_none());
    }
}
