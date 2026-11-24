use std::io::Cursor;

pub(super) const MAX_IMAGE: usize = 25 * 1024 * 1024;
const MAX_PIXELS: usize = 16_000_000;

#[derive(Clone)]
pub(super) struct Bitmap {
    pub width: usize,
    pub height: usize,
    // Top-down BGRA, suitable for a 32-bit Windows DIB.
    pub pixels: Vec<u8>,
}

impl Bitmap {
    pub fn size(w: usize, h: usize) -> Result<usize, String> {
        let n = w.checked_mul(h).filter(|&n| w > 0 && h > 0 && n <= MAX_PIXELS);
        n.map(|v| v * 4).ok_or_else(|| "Image exceeds the 16 megapixel limit.".into())
    }

    pub fn from_png(bytes: &[u8]) -> Result<Self, String> {
        if bytes.len() > MAX_IMAGE { return Err("Image exceeds the 25 MB limit.".into()); }
        let mut decoder = png::Decoder::new(Cursor::new(bytes));
        decoder.set_transformations(png::Transformations::EXPAND | png::Transformations::STRIP_16);
        let mut reader = decoder.read_info().map_err(|e| e.to_string())?;
        let (w, h) = (reader.info().width as usize, reader.info().height as usize);
        Self::size(w, h)?;
        let mut raw = vec![0; reader.output_buffer_size()];
        let info = reader.next_frame(&mut raw).map_err(|e| e.to_string())?;
        let (w,h)=(info.width as usize,info.height as usize);
        let len=Self::size(w,h)?;
        let channels = info.color_type.samples();
        if info.buffer_size()!=w*h*channels || info.buffer_size()>raw.len(){
            return Err("Invalid decoded image dimensions.".into());
        }
        let mut pixels = Vec::with_capacity(len);
        for p in raw[..info.buffer_size()].chunks_exact(channels) {
            let bgra = match info.color_type {
                png::ColorType::Rgba => [p[2],p[1],p[0],p[3]],
                png::ColorType::Rgb => [p[2],p[1],p[0],255],
                png::ColorType::Grayscale => [p[0],p[0],p[0],255],
                png::ColorType::GrayscaleAlpha => [p[0],p[0],p[0],p[1]],
                _ => return Err("Unsupported PNG palette.".into()),
            };
            pixels.extend_from_slice(&bgra);
        }
        Ok(Self {width:w, height:h, pixels})
    }

    pub fn png(&self) -> Result<Vec<u8>, String> {
        let mut encoded = Vec::new();
        {
            let mut encoder = png::Encoder::new(&mut encoded,self.width as u32,self.height as u32);
            encoder.set_color(png::ColorType::Rgba);
            encoder.set_depth(png::BitDepth::Eight);
            encoder.set_compression(png::Compression::Fast);
            let mut writer = encoder.write_header().map_err(|e| e.to_string())?;
            let mut rgba = self.pixels.clone();
            for p in rgba.chunks_exact_mut(4) {p.swap(0,2);}
            writer.write_image_data(&rgba).map_err(|e| e.to_string())?;
        }
        if encoded.len()>MAX_IMAGE {return Err("Encoded image exceeds the 25 MB limit.".into());}
        Ok(encoded)
    }

    pub fn from_dib(data: &[u8]) -> Result<Self,String> {
        let u32_at = |o:usize| -> Result<u32,String> {
            let s=data.get(o..o+4).ok_or("Truncated bitmap header.")?;
            Ok(u32::from_le_bytes(s.try_into().unwrap()))
        };
        let header=u32_at(0)? as usize;
        if header<40 || header>data.len() {return Err("Unsupported bitmap header.".into());}
        let w=u32_at(4)? as i32;
        let signed_h=u32_at(8)? as i32;
        if w<=0 || signed_h==i32::MIN || u16::from_le_bytes([data[12],data[13]])!=1 {return Err("Invalid bitmap dimensions.".into());}
        let (width,height)=(w as usize,signed_h.unsigned_abs() as usize);
        let len=Self::size(width,height)?;
        let bpp=u16::from_le_bytes([data[14],data[15]]) as usize;
        let compression=u32_at(16)?;
        if !matches!((bpp,compression),(24|32,0)|(32,3|6)) {return Err("Unsupported clipboard pixel format.".into());}
        let masks=if compression!=0 {
            let alpha=if header>=56 || compression==6 {u32_at(52)?}else{0};
            [u32_at(40)?,u32_at(44)?,u32_at(48)?,alpha]
        }else{[0;4]};
        let mask_len=if compression!=0 && header==40 {if compression==6 {16}else{12}}else{0};
        let offset=header.checked_add(mask_len).and_then(|n| n.checked_add((u32_at(32).ok()? as usize).checked_mul(4)?)).ok_or("Invalid bitmap offset.")?;
        let stride=(width*bpp).div_ceil(32)*4;
        let needed=offset.checked_add(stride*height).ok_or("Invalid bitmap length.")?;
        if data.len()<needed {return Err("Truncated clipboard pixels.".into());}
        let unpack=|word:u32,mask:u32,default:u8| {
            if mask==0{return default;}
            let shift=mask.trailing_zeros();
            let sample = ((word & mask) >> shift) as u64;
            ((sample * 255) / ((mask >> shift) as u64)) as u8
        };
        let mut pixels=Vec::with_capacity(len);
        for y in 0..height {
            let start=offset+if signed_h>0 {(height-1-y)*stride}else{y*stride};
            for p in data[start..start+width*(bpp/8)].chunks_exact(bpp/8) {
                let pixel=if compression==0 {[p[0],p[1],p[2],255]}else{
                    let v=u32::from_le_bytes(p.try_into().unwrap());
                    [unpack(v,masks[2],0),unpack(v,masks[1],0),unpack(v,masks[0],0),unpack(v,masks[3],255)]
                };
                pixels.extend_from_slice(&pixel);
            }
        }
        Ok(Self {width,height,pixels})
    }

    pub fn dib(&self)->Vec<u8> {
        let mut result=vec![0u8;40];
        result[0..4].copy_from_slice(&40u32.to_le_bytes());
        result[4..8].copy_from_slice(&(self.width as i32).to_le_bytes());
        result[8..12].copy_from_slice(&(-(self.height as i32)).to_le_bytes());
        result[12..14].copy_from_slice(&1u16.to_le_bytes());
        result[14..16].copy_from_slice(&32u16.to_le_bytes());
        result.extend_from_slice(&self.pixels);
        result
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test] fn dib_and_png_pixel_order() {
        let im=Bitmap{width:2,height:1,pixels:vec![1,2,3,255,4,5,6,255]};
        assert_eq!(Bitmap::from_dib(&im.dib()).unwrap().pixels,im.pixels);
        assert_eq!(Bitmap::from_png(&im.png().unwrap()).unwrap().pixels,im.pixels);
        assert!(Bitmap::from_dib(&[0u8;8]).is_err());
        assert!(Bitmap::size(1,16_000_001).is_err());
    }
}
