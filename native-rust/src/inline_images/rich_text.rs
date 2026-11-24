// Rich Edit handles typography and paragraph layout. Only passive formatting
// and visible text cross the clipboard boundary; images use Buddy's PNG store.
use super::*;
use std::fmt::Write as _;
use html5gum::{Token, Tokenizer};

const MAX_RTF: usize = 8 * 1024 * 1024;
pub(crate) fn format_key(id: &str) -> String { format!("note-format/{id}") }
pub(crate) fn saved(records: &crate::storage::Records, id: &str) -> Option<String> {
    records.get(&format_key(id))?.first()?.first().cloned()
}

unsafe extern "system" fn output(cookie:usize, data:*mut u8, count:i32, used:*mut i32)->u32 {
    let out=&mut *(cookie as *mut Vec<u8>);
    if count<0 || out.len().saturating_add(count as usize)>MAX_RTF {return 1;}
    out.extend_from_slice(std::slice::from_raw_parts(data,count as usize));*used=count;0
}
unsafe extern "system" fn input(cookie:usize, data:*mut u8, count:i32, used:*mut i32)->u32 {
    let remaining=&mut *(cookie as *mut &[u8]);
    if count<0{return 1;}
    let len=remaining.len().min(count as usize);
    std::ptr::copy_nonoverlapping(remaining.as_ptr(),data,len);
    *remaining=&remaining[len..];*used=len as i32;0
}
pub(super) fn snapshot(h:HWND)->Result<String,String>{
    let mut bytes=Vec::<u8>::new();
    let mut stream=EDITSTREAM{dwCookie:&mut bytes as *mut _ as _,pfnCallback:Some(output),..Default::default()};
    unsafe{SendMessageW(h,EM_STREAMOUT,SF_RTFNOOBJS as _,&mut stream as *mut _ as _);}
    if stream.dwError!=0{return Err("Could not save the note formatting.".into());}
    if bytes.last()==Some(&0){bytes.pop();}
    String::from_utf8(bytes).map_err(|_|"Could not encode the note formatting.".into())
}
pub(super) unsafe fn insert_rtf(h:HWND, rtf:&[u8], at_selection:bool)->bool {
    let mut bytes=rtf;
    let mut stream=EDITSTREAM{dwCookie:&mut bytes as *mut _ as _,pfnCallback:Some(input),..Default::default()};
    let flags=SF_RTF | if at_selection{SFF_SELECTION}else{0};
    SendMessageW(h,EM_STREAMIN,flags as _,&mut stream as *mut _ as _);
    stream.dwError==0
}

// Preserve the literal text, Unicode escapes, font/color tables and document
// styling without handing field instructions, pictures or OLE data to Rich Edit.
fn passive_word(word:&[u8])->bool {
    matches!(word,b"rtf"|b"ansi"|b"ansicpg"|b"uc"|b"u"|b"deff"|b"deflang"|b"fonttbl"|b"colortbl"|
        b"f"|b"fcharset"|b"fnil"|b"froman"|b"fswiss"|b"fmodern"|b"fscript"|b"fdecor"|b"ftech"|b"fbidi"|
        b"red"|b"green"|b"blue"|b"cf"|b"cb"|b"highlight"|b"fs"|b"b"|b"i"|b"ul"|b"ulnone"|b"uld"|
        b"uldb"|b"ulw"|b"strike"|b"super"|b"sub"|b"nosupersub"|b"plain"|b"pard"|b"par"|b"line"|b"tab"|
        b"ql"|b"qr"|b"qc"|b"qj"|b"li"|b"ri"|b"fi"|b"sb"|b"sa"|b"sl"|b"slmult"|b"tx"|b"rtlpar"|
        b"ltrpar"|b"rtlch"|b"ltrch"|b"lang"|b"bullet"|b"emdash"|b"endash"|b"lquote"|b"rquote"|b"ldblquote"|b"rdblquote")
}
fn clean_rtf(data:&[u8])->Option<Vec<u8>> {
    if data.len()>MAX_RTF || !data.starts_with(br"{\rtf1"){return None;}
    let mut out=Vec::with_capacity(data.len());let mut stack=Vec::new();
    let mut skip=false;let mut first=false;let mut i=0;
    while i<data.len(){
        let ch=data[i];i+=1;
        match ch{
            b'{'=>{if stack.len()==128{return None;}stack.push((skip,out.len()));first=true;if !skip{out.push(ch);}}
            b'}'=>{let (parent,at)=stack.pop()?;if skip{out.truncate(at);}else{out.push(ch);}skip=parent;first=false;if stack.is_empty(){return Some(out);}}
            b'\\'=>{
                let start=i-1;let sym=*data.get(i)?;i+=1;
                if !sym.is_ascii_alphabetic(){
                    if sym==b'*' && first {skip=true;continue;}
                    if sym==b'\''{if !data.get(i..i+2)?.iter().all(u8::is_ascii_hexdigit){return None;}i+=2;}
                    if !skip && matches!(sym,b'\\'|b'{'|b'}'|b'\''|b'~'|b'-'|b'_'){out.extend_from_slice(&data[start..i]);}
                    first=false;continue;
                }
                while data.get(i).is_some_and(u8::is_ascii_alphabetic){i+=1;}
                let word=&data[start+1..i];let number=i;
                if data.get(i)==Some(&b'-'){i+=1;}
                while data.get(i).is_some_and(u8::is_ascii_digit){i+=1;}
                let end=i;if data.get(i)==Some(&b' '){i+=1;}
                // SF_RTFNOOBJS emits a placeholder plus one encoded space
                // for each static object. Reinsert the PNG at its saved offset.
                if word==b"objattph" && data[i..].starts_with(br"\'20"){i+=4;}
                if word==b"bin"{let n=std::str::from_utf8(&data[number..end]).ok()?.parse::<usize>().ok()?;i=i.checked_add(n)?;if i>data.len(){return None;}skip=true;}
                if matches!(word,b"object"|b"pict"|b"shp"|b"shppict"|b"nonshppict"|b"info"|b"stylesheet"|b"filetbl"|b"fldinst"|b"datastore"|b"header"|b"footer"|b"listtable"|b"listoverridetable"|b"generator"){skip=true;}
                if !skip && passive_word(word){out.extend_from_slice(&data[start..end]);out.push(b' ');}
                first=false;
            }
            b'\r'|b'\n'=>{}
            0=>return None,
            _=>{first=false;if !skip{out.push(ch);}}
        }
        if out.len()>MAX_RTF{return None;}
    }
    None
}

fn escape(text:&str,out:&mut String){
    for unit in text.encode_utf16(){match unit{
        92|123|125=>{out.push('\\');out.push(unit as u8 as char);}
        9=>out.push_str("\\tab "),10|13=>out.push_str("\\line "),
        32..=126=>out.push(unit as u8 as char),
        _=>{let _=write!(out,"\\u{}?",unit as i16);}
    }}
}
fn color(s:&str)->Option<[u8;3]>{
    let named=match s{"red"=>"ff0000","blue"=>"0000ff","green"=>"008000","black"=>"000000","white"=>"ffffff","yellow"=>"ffff00","gray"|"grey"=>"808080","purple"=>"800080","orange"=>"ffa500",_=>""};
    if let Some(rgb)=s.strip_prefix("rgb(").and_then(|s|s.strip_suffix(')')){
        let mut v=rgb.split(',').map(|x|x.trim().parse().ok());let c=[v.next()??,v.next()??,v.next()??];return v.next().is_none().then_some(c);
    }
    let code=s.strip_prefix('#').unwrap_or(named);let bytes=u32::from_str_radix(code,16).ok()?;
    Some(match code.len(){3=>[((bytes>>8)as u8&15)*17,((bytes>>4)as u8&15)*17,(bytes as u8&15)*17],6=>[(bytes>>16)as u8,(bytes>>8)as u8,bytes as u8],_=>return None})
}
fn css(body:&mut String,colors:&mut Vec<[u8;3]>,fonts:&mut Vec<String>,name:&str,value:&str){
    let value=value.trim().trim_end_matches("!important").trim();
    match name.trim(){
        "font-weight"=>body.push_str(if value=="bold"||value=="bolder"||value.parse::<u16>().is_ok_and(|n|n>=600){"\\b "}else{"\\b0 "}),
        "font-style"=>body.push_str(if matches!(value,"italic"|"oblique"){"\\i "}else{"\\i0 "}),
        "text-decoration"|"text-decoration-line"=>{if value.contains("underline"){body.push_str("\\ul ");}if value.contains("line-through"){body.push_str("\\strike ");}if value=="none"{body.push_str("\\ulnone \\strike0 ");}}
        "color"|"background-color"=>if let Some(c)=color(value){if colors.len()<256||colors.contains(&c){let index=colors.iter().position(|x|*x==c).unwrap_or_else(||{colors.push(c);colors.len()-1});let _=write!(body,"\\{}{} ",if name.trim()=="color"{"cf"}else{"highlight"},index+1);}},
        "font-size"=>{let size=value.strip_suffix("pt").and_then(|n|n.parse::<f32>().ok()).or_else(||value.strip_suffix("px")?.parse::<f32>().ok().map(|n|n*0.75));if let Some(size)=size.filter(|n|n.is_finite() && *n>0.){let _=write!(body,"\\fs{} ",(size.clamp(5.,144.)*2.).round()as u16);}},
        "font-family"=>{let name=value.split(',').next().unwrap_or("").trim().trim_matches(['\'','"']);if !name.is_empty()&&name.len()<=64 && !name.contains(';') && fonts.len()<128{let i=fonts.iter().position(|f|f==name).unwrap_or_else(||{fonts.push(name.into());fonts.len()-1});let _=write!(body,"\\f{i} ");}},
        "text-align"=>body.push_str(match value{"center"=>"\\qc ","right"=>"\\qr ","justify"=>"\\qj ",_=>"\\ql "}),
        _=>{}
    }
}
fn fragment(data:&[u8])->Option<(&str,&str)>{
    let text=std::str::from_utf8(data.split(|b|*b==0).next()?).ok()?;
    let offset=|key:&str|text.lines().take(10).find_map(|l|l.strip_prefix(key))?.trim().parse::<usize>().ok();
    let start_html=offset("StartHTML:").or_else(||text.find('<'))?;
    if let (Some(a),Some(b))=(offset("StartFragment:"),offset("EndFragment:")){
        if let (Some(prefix),Some(fragment))=(text.get(start_html..a),text.get(a..b)){return Some((prefix,fragment));}
    }
    let html=text.get(start_html..)?;
    if let Some((prefix,s))=html.split_once("<!--StartFragment-->"){
        return s.split_once("<!--EndFragment-->").map(|(fragment,_)|(prefix,fragment));
    }
    Some(("",html))
}
fn void_tag(name:&[u8])->bool{
    matches!(name,b"br"|b"img"|b"hr"|b"meta"|b"input"|b"link"|b"base"|b"area"|b"source"|b"wbr"|b"col"|b"param"|b"embed"|b"track")
}
fn html_rtf(data:&[u8])->Option<Vec<u8>>{
    struct Frame {name:String, hidden:bool, block:bool, pre:bool, href:Option<String>, start:usize, number:usize}
    let mut stack:Vec<Frame>=Vec::new();let mut body=String::new();let mut plain=String::new();let mut colors=Vec::new();let mut fonts=vec!["Segoe UI".to_string()];let mut newline=false;
    let (prefix,selection)=fragment(data)?;
    // A browser can put the selected text inside a styled ancestor outside
    // StartFragment. Carry open elements into the selection, discarding all
    // surrounding document text and unrelated closed siblings.
    let mut parents:Vec<html5gum::StartTag<()>>=Vec::new();
    for token in Tokenizer::new(prefix).flatten(){match token{
        Token::StartTag(tag) if !tag.self_closing && !void_tag(&tag.name)=>{
            if parents.len()>=128{return None;} parents.push(tag);
        }
        Token::EndTag(tag)=>if let Some(i)=parents.iter().rposition(|p|p.name==tag.name){parents.truncate(i);},
        _=>{}
    }}
    let tokens=parents.into_iter().map(Token::StartTag).chain(Tokenizer::new(selection).flatten());
    for token in tokens{
        match token{
            Token::StartTag(t)=>{
                let name=String::from_utf8(t.name.0).ok()?;
                let hidden=stack.last().is_some_and(|p|p.hidden)||matches!(name.as_str(),"script"|"style"|"head"|"iframe"|"object"|"template"|"svg");
                if name=="br"{if !hidden{body.push_str("\\line ");plain.push('\n');newline=false;}continue;}
                if void_tag(name.as_bytes()){continue;}
                if stack.len()>=128{return None;}
                let block=matches!(name.as_str(),"p"|"div"|"li"|"tr"|"blockquote"|"pre"|"h1"|"h2"|"h3"|"h4"|"h5"|"h6");
                let attr=|n:&str|t.attributes.iter().find(|(k,_)|k.as_slice()==n.as_bytes()).and_then(|(_,v)|std::str::from_utf8(&v.value).ok());
                if !hidden{
                    if (newline||block)&&!plain.is_empty()&&!plain.ends_with('\n'){body.push_str("\\par ");plain.push('\n');}newline=false;
                    body.push('{');body.push_str(match name.as_str(){"strong"|"b"=>"\\b ","em"|"i"=>"\\i ","u"|"ins"=>"\\ul ","s"|"del"|"strike"=>"\\strike ","sub"=>"\\sub ","sup"=>"\\super ","h1"=>"\\b\\fs40 ","h2"=>"\\b\\fs32 ","h3"|"h4"|"h5"|"h6"=>"\\b\\fs26 ",_=>""});
                    if name=="li"{let list=stack.iter_mut().rev().find(|f|matches!(f.name.as_str(),"ol"|"ul"));let bullet=match list{Some(f) if f.name=="ol"=>{let v=format!("{}. ",f.number);f.number=f.number.saturating_add(1);v},_=>"• ".into()};escape(&bullet,&mut body);plain.push_str(&bullet);}
                    if let Some(style)=attr("style"){for pair in style.split(';'){if let Some((k,v))=pair.split_once(':'){css(&mut body,&mut colors,&mut fonts,&k.to_ascii_lowercase(),v);}}}
                    if name=="font"{if let Some(c)=attr("color"){css(&mut body,&mut colors,&mut fonts,"color",c);}if let Some(f)=attr("face"){css(&mut body,&mut colors,&mut fonts,"font-family",f);}}
                }
                let pre=name=="pre"||stack.last().is_some_and(|p|p.pre);
                let href=attr("href").filter(|s|name=="a"&&workspace::valid_url(s)).map(str::to_owned);
                if t.self_closing{if !hidden{body.push('}');}}else{stack.push(Frame{name,hidden,block,pre,href,start:plain.len(),number:attr("start").and_then(|s|s.parse().ok()).unwrap_or(1)});}
            }
            Token::EndTag(t)=>if let Some(i)=stack.iter().rposition(|f|f.name.as_bytes()==t.name.as_slice()){
                while stack.len()>i{let f=stack.pop()?;if !f.hidden{
                    if let Some(url)=f.href{if plain[f.start..].trim()!=url{let link=format!(" ({url})");escape(&link,&mut body);plain.push_str(&link);}}
                    body.push('}');newline|=f.block;
                }}
            }
            Token::String(t) if !stack.last().is_some_and(|p|p.hidden)=>{
                let value=std::str::from_utf8(&t.value).ok()?;
                if newline&&value.trim().is_empty(){continue;}
                if newline&&!plain.ends_with('\n'){body.push_str("\\par ");plain.push('\n');}newline=false;
                for c in value.chars(){if c.is_ascii_whitespace()&&!stack.last().is_some_and(|p|p.pre){if !plain.is_empty()&&!plain.ends_with([' ','\n']){body.push(' ');plain.push(' ');}}else{escape(c.encode_utf8(&mut [0;4]),&mut body);plain.push(c);}}
            }
            _=>{}
        }
        if plain.len()>MAX_TEXT*4 || body.len()>MAX_RTF{return None;}
    }
    if plain.trim().is_empty(){return None;}
    for f in stack.into_iter().rev(){if !f.hidden{body.push('}');}}
    let mut rtf=String::from("{\\rtf1\\ansi\\uc1\\deff0{\\fonttbl");
    for (i,name) in fonts.iter().enumerate(){let _=write!(rtf,"{{\\f{i} ");escape(name,&mut rtf);rtf.push_str(";}");}
    rtf.push_str("}{\\colortbl;");for [r,g,b] in colors{let _=write!(rtf,"\\red{r}\\green{g}\\blue{b};");}
    rtf.push_str("}\\f0\\fs20 ");rtf.push_str(&body);rtf.push('}');Some(rtf.into_bytes())
}
pub(super) fn clipboard_rtf(h:HWND)->Option<Vec<u8>>{
    let (html,rtf)=workspace::formatted_clipboard(h,MAX_RTF)?;
    // Word's RTF carries full paragraph and font tables. HTML is the common
    // format for selections copied from a browser.
    rtf.as_deref().and_then(clean_rtf).or_else(||html.as_deref().and_then(html_rtf))
}
pub(super) fn restore(h:HWND,rtf:&str,body:&str)->bool{
    let Some(clean)=clean_rtf(rtf.as_bytes())else{return false};
    (unsafe{insert_rtf(h,&clean,false)}) && normalized(&String::from_utf16_lossy(&raw_text(h)))==normalized(body)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn clipboard_fragment_keeps_ancestor_formatting_and_excludes_surrounding_text() {
        let before = "<html><body><p>Outside</p><strong style='color:#123456'><!--StartFragment-->";
        let selected = "é &amp; <i>friends</i>";
        let after = "<!--EndFragment--></strong><p>Outside again</p></body></html>";
        let header = |start, end, frag, stop| format!(
            "Version:1.0\r\nStartHTML:{start:010}\r\nEndHTML:{end:010}\r\nStartFragment:{frag:010}\r\nEndFragment:{stop:010}\r\n"
        );
        let start = header(0, 0, 0, 0).len();
        let frag = start + before.len();
        let stop = frag + selected.len();
        let data = header(start, stop + after.len(), frag, stop) + before + selected + after;
        let markup = String::from_utf8(html_rtf(data.as_bytes()).unwrap()).unwrap();
        assert!(markup.contains(r"\b ") && markup.contains(r"\i "));
        assert!(markup.contains(r"\red18\green52\blue86;"));
        assert!(markup.contains(r"\u233? & ") && markup.contains("friends"));
        assert!(!markup.contains("Outside"));
    }

    #[test]
    fn rtf_import_keeps_visible_text_and_removes_attachments_and_instructions() {
        let raw = br"{\rtf1\ansi {\b label} {\field{\*\fldinst PRIVATE}{\fldrslt link}}{\object{\*\objdata PRIVATE}}{\pict PRIVATE}\\objattph}";
        let clean = String::from_utf8(clean_rtf(raw).unwrap()).unwrap();
        assert!(clean.contains("label") && clean.contains("link"));
        assert!(clean.contains(r"\\objattph"));
        assert!(!clean.contains("PRIVATE"));
        assert!(clean_rtf(br"{\rtf1 {\pict\bin9999 x}}").is_none());
    }
}
