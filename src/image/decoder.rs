//! Image Decoder
//!
//! Handles decoding of various image formats (PNG, JPEG, GIF, etc.)
//! using the `image` crate.

use super::ImageError;
use image::{imageops::FilterType, DynamicImage, GenericImageView, ImageReader};
use std::io::Cursor;

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum ImageFormat {
    Png,
    Jpeg,
    Gif,
    Webp,
    Bmp,
    Tiff,
    Rgba,
    Rgb,
}

impl ImageFormat {
    pub fn from_extension(ext: &str) -> Option<Self> {
        match ext.to_lowercase().as_str() {
            "png" => Some(Self::Png),
            "jpg" | "jpeg" => Some(Self::Jpeg),
            "gif" => Some(Self::Gif),
            "webp" => Some(Self::Webp),
            "bmp" => Some(Self::Bmp),
            "tiff" | "tif" => Some(Self::Tiff),
            _ => None,
        }
    }

    pub fn from_kitty_format(format: u32) -> Option<Self> {
        match format {
            24 => Some(Self::Png),
            32 => Some(Self::Rgba),
            33 => Some(Self::Rgb),
            100 => Some(Self::Jpeg),
            101 => Some(Self::Gif),
            102 => Some(Self::Webp),
            103 => Some(Self::Bmp),
            104 => Some(Self::Tiff),
            _ => None,
        }
    }

    pub fn to_image_crate_format(&self) -> image::ImageFormat {
        match self {
            Self::Png => image::ImageFormat::Png,
            Self::Jpeg => image::ImageFormat::Jpeg,
            Self::Gif => image::ImageFormat::Gif,
            Self::Webp => image::ImageFormat::WebP,
            Self::Bmp => image::ImageFormat::Bmp,
            Self::Tiff => image::ImageFormat::Tiff,
            Self::Rgba | Self::Rgb => image::ImageFormat::Png,
        }
    }

    pub fn mime_type(&self) -> &'static str {
        match self {
            Self::Png => "image/png",
            Self::Jpeg => "image/jpeg",
            Self::Gif => "image/gif",
            Self::Webp => "image/webp",
            Self::Bmp => "image/bmp",
            Self::Tiff => "image/tiff",
            Self::Rgba => "application/octet-stream",
            Self::Rgb => "application/octet-stream",
        }
    }
}

#[derive(Debug, Clone)]
pub struct DecodedImage {
    pub width: u32,
    pub height: u32,
    pub rgba_data: Vec<u8>,
}

impl DecodedImage {
    pub fn new(width: u32, height: u32, rgba_data: Vec<u8>) -> Self {
        Self {
            width,
            height,
            rgba_data,
        }
    }

    pub fn from_dynamic_image(img: DynamicImage) -> Self {
        let rgba = img.to_rgba8();
        let (width, height) = rgba.dimensions();
        Self {
            width,
            height,
            rgba_data: rgba.into_raw(),
        }
    }

    pub fn get_pixel(&self, x: u32, y: u32) -> Option<[u8; 4]> {
        if x >= self.width || y >= self.height {
            return None;
        }

        let idx = ((y * self.width + x) * 4) as usize;
        if idx + 4 > self.rgba_data.len() {
            return None;
        }

        Some([
            self.rgba_data[idx],
            self.rgba_data[idx + 1],
            self.rgba_data[idx + 2],
            self.rgba_data[idx + 3],
        ])
    }

    pub fn size_bytes(&self) -> usize {
        self.rgba_data.len()
    }
}

pub struct ImageDecoder {
    max_width: u32,
    max_height: u32,
    max_file_size: usize,
}

impl ImageDecoder {
    pub fn new() -> Self {
        Self {
            max_width: 4096,
            max_height: 4096,
            max_file_size: 100 * 1024 * 1024,
        }
    }

    pub fn with_limits(max_width: u32, max_height: u32, max_file_size: usize) -> Self {
        Self {
            max_width,
            max_height,
            max_file_size,
        }
    }

    pub fn decode(&self, data: &[u8], format: ImageFormat) -> Result<DecodedImage, ImageError> {
        if data.len() > self.max_file_size {
            return Err(ImageError::Decode(format!(
                "File too large: {} bytes (max: {})",
                data.len(),
                self.max_file_size
            )));
        }

        if format == ImageFormat::Rgba || format == ImageFormat::Rgb {
            return Err(ImageError::Decode(
                "Raw RGBA/RGB data requires dimensions. Use decode_rgba() or decode_rgb() instead."
                    .to_string(),
            ));
        }

        let cursor = Cursor::new(data);
        let reader = ImageReader::with_format(cursor, format.to_image_crate_format());

        let img = reader
            .decode()
            .map_err(|e| ImageError::Decode(format!("Failed to decode image: {}", e)))?;

        let (width, height) = img.dimensions();

        if width > self.max_width || height > self.max_height {
            return Err(ImageError::InvalidDimensions(width, height));
        }

        Ok(DecodedImage::from_dynamic_image(img))
    }

    pub fn decode_auto(&self, data: &[u8]) -> Result<DecodedImage, ImageError> {
        if data.len() > self.max_file_size {
            return Err(ImageError::Decode(format!(
                "File too large: {} bytes (max: {})",
                data.len(),
                self.max_file_size
            )));
        }

        let img = image::load_from_memory(data)
            .map_err(|e| ImageError::Decode(format!("Failed to decode image: {}", e)))?;

        let (width, height) = img.dimensions();

        if width > self.max_width || height > self.max_height {
            return Err(ImageError::InvalidDimensions(width, height));
        }

        Ok(DecodedImage::from_dynamic_image(img))
    }

    pub fn decode_png(&self, data: &[u8]) -> Result<DecodedImage, ImageError> {
        self.decode(data, ImageFormat::Png)
    }

    pub fn decode_jpeg(&self, data: &[u8]) -> Result<DecodedImage, ImageError> {
        self.decode(data, ImageFormat::Jpeg)
    }

    pub fn decode_gif(&self, data: &[u8]) -> Result<DecodedImage, ImageError> {
        self.decode(data, ImageFormat::Gif)
    }

    pub fn decode_webp(&self, data: &[u8]) -> Result<DecodedImage, ImageError> {
        self.decode(data, ImageFormat::Webp)
    }

    pub fn decode_rgba(
        &self,
        data: &[u8],
        width: u32,
        height: u32,
    ) -> Result<DecodedImage, ImageError> {
        // Dimensões vêm do protocolo (saída do terminal): valida antes e
        // calcula o tamanho sem estourar u32
        if width > self.max_width || height > self.max_height {
            return Err(ImageError::InvalidDimensions(width, height));
        }

        let expected_size = (width as usize)
            .checked_mul(height as usize)
            .and_then(|pixels| pixels.checked_mul(4))
            .ok_or(ImageError::InvalidDimensions(width, height))?;
        if data.len() != expected_size {
            return Err(ImageError::Decode(format!(
                "Invalid RGBA data size: {} (expected: {})",
                data.len(),
                expected_size
            )));
        }

        Ok(DecodedImage::new(width, height, data.to_vec()))
    }

    pub fn decode_rgb(
        &self,
        data: &[u8],
        width: u32,
        height: u32,
    ) -> Result<DecodedImage, ImageError> {
        // Dimensões vêm do protocolo (saída do terminal): valida antes e
        // calcula o tamanho sem estourar u32
        if width > self.max_width || height > self.max_height {
            return Err(ImageError::InvalidDimensions(width, height));
        }

        let expected_size = (width as usize)
            .checked_mul(height as usize)
            .and_then(|pixels| pixels.checked_mul(3))
            .ok_or(ImageError::InvalidDimensions(width, height))?;
        if data.len() != expected_size {
            return Err(ImageError::Decode(format!(
                "Invalid RGB data size: {} (expected: {})",
                data.len(),
                expected_size
            )));
        }

        let rgba_data: Vec<u8> = data
            .chunks_exact(3)
            .flat_map(|rgb| [rgb[0], rgb[1], rgb[2], 255])
            .collect();

        Ok(DecodedImage::new(width, height, rgba_data))
    }

    pub fn get_dimensions(
        &self,
        data: &[u8],
        format: ImageFormat,
    ) -> Result<(u32, u32), ImageError> {
        if format == ImageFormat::Rgba || format == ImageFormat::Rgb {
            return Err(ImageError::Decode(
                "Cannot determine dimensions of raw RGBA/RGB data without explicit width/height"
                    .to_string(),
            ));
        }

        let cursor = Cursor::new(data);
        let reader = ImageReader::with_format(cursor, format.to_image_crate_format());

        let (width, height) = reader
            .into_dimensions()
            .map_err(|e| ImageError::Decode(format!("Failed to read image dimensions: {}", e)))?;

        Ok((width, height))
    }

    pub fn get_dimensions_auto(&self, data: &[u8]) -> Result<(u32, u32), ImageError> {
        let img = image::load_from_memory(data)
            .map_err(|e| ImageError::Decode(format!("Failed to read image dimensions: {}", e)))?;

        Ok(img.dimensions())
    }

    pub fn resize(
        &self,
        image: &DecodedImage,
        max_width: u32,
        max_height: u32,
    ) -> Result<DecodedImage, ImageError> {
        let (new_width, new_height) =
            resize_to_fit(image.width, image.height, max_width, max_height);

        if new_width == image.width && new_height == image.height {
            return Ok(image.clone());
        }

        let img_buffer =
            image::RgbaImage::from_raw(image.width, image.height, image.rgba_data.clone())
                .ok_or_else(|| ImageError::Decode("Failed to create image buffer".to_string()))?;

        let resized =
            image::imageops::resize(&img_buffer, new_width, new_height, FilterType::Lanczos3);

        Ok(DecodedImage::new(new_width, new_height, resized.into_raw()))
    }
}

impl Default for ImageDecoder {
    fn default() -> Self {
        Self::new()
    }
}

pub fn resize_to_fit(width: u32, height: u32, max_width: u32, max_height: u32) -> (u32, u32) {
    if width == 0 || height == 0 {
        return (0, 0);
    }

    if width <= max_width && height <= max_height {
        return (width, height);
    }

    let aspect = width as f64 / height as f64;
    let max_aspect = max_width as f64 / max_height as f64;

    if aspect > max_aspect {
        let new_width = max_width;
        let new_height = (max_width as f64 / aspect).round() as u32;
        (new_width, new_height.max(1))
    } else {
        let new_height = max_height;
        let new_width = (max_height as f64 * aspect).round() as u32;
        (new_width.max(1), new_height)
    }
}

pub fn resize_to_fill(
    width: u32,
    height: u32,
    target_width: u32,
    target_height: u32,
) -> (u32, u32, u32, u32) {
    if width == 0 || height == 0 {
        return (0, 0, 0, 0);
    }

    let aspect = width as f64 / height as f64;
    let target_aspect = target_width as f64 / target_height as f64;

    if aspect > target_aspect {
        let new_width = (target_height as f64 * aspect).round() as u32;
        let new_height = target_height;
        let offset_x = ((new_width - target_width) / 2).saturating_sub(1);
        (new_width, new_height, offset_x, 0)
    } else {
        let new_width = target_width;
        let new_height = (target_width as f64 / aspect).round() as u32;
        let offset_y = ((new_height - target_height) / 2).saturating_sub(1);
        (new_width, new_height, 0, offset_y)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn create_test_png() -> Vec<u8> {
        let img = image::DynamicImage::ImageRgba8(image::RgbaImage::from_pixel(
            10,
            10,
            image::Rgba([255, 0, 0, 255]),
        ));
        let mut buf = Vec::new();
        img.write_to(&mut std::io::Cursor::new(&mut buf), image::ImageFormat::Png)
            .unwrap();
        buf
    }

    fn create_test_jpeg() -> Vec<u8> {
        let img = image::DynamicImage::ImageRgb8(image::RgbImage::from_pixel(
            10,
            10,
            image::Rgb([0, 255, 0]),
        ));
        let mut buf = Vec::new();
        img.write_to(
            &mut std::io::Cursor::new(&mut buf),
            image::ImageFormat::Jpeg,
        )
        .unwrap();
        buf
    }

    #[test]
    fn test_format_from_extension() {
        assert_eq!(ImageFormat::from_extension("png"), Some(ImageFormat::Png));
        assert_eq!(ImageFormat::from_extension("PNG"), Some(ImageFormat::Png));
        assert_eq!(ImageFormat::from_extension("jpg"), Some(ImageFormat::Jpeg));
        assert_eq!(ImageFormat::from_extension("jpeg"), Some(ImageFormat::Jpeg));
        assert_eq!(ImageFormat::from_extension("gif"), Some(ImageFormat::Gif));
        assert_eq!(ImageFormat::from_extension("unknown"), None);
    }

    #[test]
    fn test_format_from_kitty_format() {
        assert_eq!(ImageFormat::from_kitty_format(24), Some(ImageFormat::Png));
        assert_eq!(ImageFormat::from_kitty_format(32), Some(ImageFormat::Rgba));
        assert_eq!(ImageFormat::from_kitty_format(100), Some(ImageFormat::Jpeg));
        assert_eq!(ImageFormat::from_kitty_format(999), None);
    }

    #[test]
    fn test_mime_types() {
        assert_eq!(ImageFormat::Png.mime_type(), "image/png");
        assert_eq!(ImageFormat::Jpeg.mime_type(), "image/jpeg");
        assert_eq!(ImageFormat::Gif.mime_type(), "image/gif");
    }

    #[test]
    fn test_decode_png() {
        let decoder = ImageDecoder::new();
        let png_data = create_test_png();

        let result = decoder.decode_png(&png_data);
        assert!(result.is_ok());

        let image = result.unwrap();
        assert_eq!(image.width, 10);
        assert_eq!(image.height, 10);
        assert_eq!(image.size_bytes(), 10 * 10 * 4);
    }

    #[test]
    fn test_decode_jpeg() {
        let decoder = ImageDecoder::new();
        let jpeg_data = create_test_jpeg();

        let result = decoder.decode_jpeg(&jpeg_data);
        assert!(result.is_ok());

        let image = result.unwrap();
        assert_eq!(image.width, 10);
        assert_eq!(image.height, 10);
    }

    #[test]
    fn test_decode_auto() {
        let decoder = ImageDecoder::new();
        let png_data = create_test_png();

        let result = decoder.decode_auto(&png_data);
        assert!(result.is_ok());

        let image = result.unwrap();
        assert_eq!(image.width, 10);
        assert_eq!(image.height, 10);
    }

    #[test]
    fn test_decode_rgba() {
        let decoder = ImageDecoder::new();
        let data = vec![255u8; 400];

        let result = decoder.decode_rgba(&data, 10, 10);
        assert!(result.is_ok());

        let image = result.unwrap();
        assert_eq!(image.width, 10);
        assert_eq!(image.height, 10);
        assert_eq!(image.rgba_data.len(), 400);
    }

    #[test]
    fn test_decode_rgba_invalid_size() {
        let decoder = ImageDecoder::new();
        let data = vec![255u8; 100];

        let result = decoder.decode_rgba(&data, 10, 10);
        assert!(result.is_err());
    }

    #[test]
    fn test_decode_rgb() {
        let decoder = ImageDecoder::new();
        let data = vec![128u8; 300];

        let result = decoder.decode_rgb(&data, 10, 10);
        assert!(result.is_ok());

        let image = result.unwrap();
        assert_eq!(image.width, 10);
        assert_eq!(image.height, 10);
        assert_eq!(image.rgba_data.len(), 400);

        assert_eq!(image.rgba_data[0], 128);
        assert_eq!(image.rgba_data[1], 128);
        assert_eq!(image.rgba_data[2], 128);
        assert_eq!(image.rgba_data[3], 255);
    }

    #[test]
    fn test_decode_rgb_invalid_size() {
        let decoder = ImageDecoder::new();
        let data = vec![255u8; 100];

        let result = decoder.decode_rgb(&data, 10, 10);
        assert!(result.is_err());
    }

    #[test]
    fn test_get_dimensions() {
        let decoder = ImageDecoder::new();
        let png_data = create_test_png();

        let result = decoder.get_dimensions(&png_data, ImageFormat::Png);
        assert!(result.is_ok());

        let (width, height) = result.unwrap();
        assert_eq!(width, 10);
        assert_eq!(height, 10);
    }

    #[test]
    fn test_get_dimensions_auto() {
        let decoder = ImageDecoder::new();
        let png_data = create_test_png();

        let result = decoder.get_dimensions_auto(&png_data);
        assert!(result.is_ok());

        let (width, height) = result.unwrap();
        assert_eq!(width, 10);
        assert_eq!(height, 10);
    }

    #[test]
    fn test_resize_to_fit() {
        assert_eq!(resize_to_fit(100, 100, 200, 200), (100, 100));
        assert_eq!(resize_to_fit(400, 100, 200, 200), (200, 50));
        assert_eq!(resize_to_fit(100, 400, 200, 200), (50, 200));
        assert_eq!(resize_to_fit(400, 400, 200, 200), (200, 200));
        assert_eq!(resize_to_fit(0, 100, 200, 200), (0, 0));
        assert_eq!(resize_to_fit(100, 0, 200, 200), (0, 0));
    }

    #[test]
    fn test_resize_to_fill() {
        let (w, h, ox, oy) = resize_to_fill(200, 100, 100, 100);
        assert_eq!(w, 200);
        assert_eq!(h, 100);
        assert_eq!(ox, 49);
        assert_eq!(oy, 0);

        let (w, h, ox, oy) = resize_to_fill(100, 200, 100, 100);
        assert_eq!(w, 100);
        assert_eq!(h, 200);
        assert_eq!(ox, 0);
        assert_eq!(oy, 49);
    }

    #[test]
    fn test_decoded_image_get_pixel() {
        let data = vec![255, 0, 0, 255, 0, 255, 0, 255];
        let image = DecodedImage::new(2, 1, data);

        assert_eq!(image.get_pixel(0, 0), Some([255, 0, 0, 255]));
        assert_eq!(image.get_pixel(1, 0), Some([0, 255, 0, 255]));
        assert_eq!(image.get_pixel(2, 0), None);
        assert_eq!(image.get_pixel(0, 1), None);
    }

    #[test]
    fn test_resize_image() {
        let decoder = ImageDecoder::new();

        let original = DecodedImage::new(100, 100, vec![255u8; 40000]);
        let resized = decoder.resize(&original, 50, 50).unwrap();

        assert_eq!(resized.width, 50);
        assert_eq!(resized.height, 50);
    }

    #[test]
    fn test_decode_exceeds_max_dimensions() {
        let decoder = ImageDecoder::with_limits(5, 5, 100 * 1024 * 1024);
        let png_data = create_test_png();

        let result = decoder.decode_png(&png_data);
        assert!(result.is_err());

        match result {
            Err(ImageError::InvalidDimensions(w, h)) => {
                assert_eq!(w, 10);
                assert_eq!(h, 10);
            }
            _ => panic!("Expected InvalidDimensions error"),
        }
    }

    #[test]
    fn test_decode_file_too_large() {
        let decoder = ImageDecoder::with_limits(4096, 4096, 10);
        let png_data = create_test_png();

        let result = decoder.decode_png(&png_data);
        assert!(result.is_err());
    }

    fn png_bytes(width: u32, height: u32) -> Vec<u8> {
        let img = image::RgbaImage::from_pixel(width, height, image::Rgba([10, 20, 30, 255]));
        let mut out = Vec::new();
        image::DynamicImage::ImageRgba8(img)
            .write_to(&mut Cursor::new(&mut out), image::ImageFormat::Png)
            .unwrap();
        out
    }

    /// Regressão: dimensões enormes vindas do protocolo estouravam
    /// `width * height * 4` em u32 e derrubavam o terminal.
    #[test]
    fn test_raw_decode_with_huge_dimensions_does_not_panic() {
        let decoder = ImageDecoder::with_limits(u32::MAX, u32::MAX, usize::MAX);
        assert!(decoder.decode_rgba(&[0; 4], 65_536, 65_536).is_err());
        assert!(decoder.decode_rgb(&[0; 3], 65_536, 65_536).is_err());
        // Com os limites padrão, a dimensão é recusada antes
        let default = ImageDecoder::default();
        assert!(matches!(
            default.decode_rgba(&[0; 4], 100_000, 1),
            Err(ImageError::InvalidDimensions(100_000, 1))
        ));
        assert!(matches!(
            default.decode_rgb(&[0; 3], 1, 100_000),
            Err(ImageError::InvalidDimensions(1, 100_000))
        ));
    }

    #[test]
    fn test_format_tables() {
        let kitty = [
            (24, ImageFormat::Png),
            (32, ImageFormat::Rgba),
            (33, ImageFormat::Rgb),
            (100, ImageFormat::Jpeg),
            (101, ImageFormat::Gif),
            (102, ImageFormat::Webp),
            (103, ImageFormat::Bmp),
            (104, ImageFormat::Tiff),
        ];
        for (code, format) in kitty {
            assert_eq!(ImageFormat::from_kitty_format(code), Some(format));
        }
        assert_eq!(ImageFormat::from_kitty_format(7), None);

        let crate_formats = [
            (ImageFormat::Png, image::ImageFormat::Png, "image/png"),
            (ImageFormat::Jpeg, image::ImageFormat::Jpeg, "image/jpeg"),
            (ImageFormat::Gif, image::ImageFormat::Gif, "image/gif"),
            (ImageFormat::Webp, image::ImageFormat::WebP, "image/webp"),
            (ImageFormat::Bmp, image::ImageFormat::Bmp, "image/bmp"),
            (ImageFormat::Tiff, image::ImageFormat::Tiff, "image/tiff"),
            (
                ImageFormat::Rgba,
                image::ImageFormat::Png,
                "application/octet-stream",
            ),
            (
                ImageFormat::Rgb,
                image::ImageFormat::Png,
                "application/octet-stream",
            ),
        ];
        for (format, expected, mime) in crate_formats {
            assert_eq!(format.to_image_crate_format(), expected, "{format:?}");
            assert_eq!(format.mime_type(), mime, "{format:?}");
        }
    }

    #[test]
    fn test_decode_limits_and_errors() {
        let png = png_bytes(8, 4);
        let decoder = ImageDecoder::new();
        let image = decoder.decode_png(&png).unwrap();
        assert_eq!((image.width, image.height), (8, 4));
        assert_eq!(image.get_pixel(0, 0), Some([10, 20, 30, 255]));
        assert_eq!(decoder.decode_auto(&png).unwrap().width, 8);
        assert_eq!(decoder.get_dimensions_auto(&png).unwrap(), (8, 4));

        // Tamanho de arquivo
        let tiny = ImageDecoder::with_limits(4096, 4096, 10);
        for result in [tiny.decode(&png, ImageFormat::Png), tiny.decode_auto(&png)] {
            let err = result.unwrap_err();
            assert!(err.to_string().contains("File too large"), "{err}");
        }

        // Dimensões
        let narrow = ImageDecoder::with_limits(4, 4, usize::MAX);
        assert!(matches!(
            narrow.decode_png(&png),
            Err(ImageError::InvalidDimensions(8, 4))
        ));
        assert!(matches!(
            narrow.decode_auto(&png),
            Err(ImageError::InvalidDimensions(8, 4))
        ));

        // Formato bruto exige dimensões explícitas
        assert!(decoder.decode(&png, ImageFormat::Rgba).is_err());
        assert!(decoder.get_dimensions(&png, ImageFormat::Rgb).is_err());

        // Dados que não são do formato
        for result in [
            decoder.decode_jpeg(b"lixo"),
            decoder.decode_gif(b"lixo"),
            decoder.decode_webp(b"lixo"),
            decoder.decode_auto(b"lixo"),
        ] {
            assert!(matches!(result, Err(ImageError::Decode(_))));
        }
        assert!(decoder.get_dimensions_auto(b"lixo").is_err());
    }

    #[test]
    fn test_raw_rgb_and_resize_paths() {
        let decoder = ImageDecoder::new();
        let rgb = decoder.decode_rgb(&[1, 2, 3, 4, 5, 6], 2, 1).unwrap();
        assert_eq!(rgb.rgba_data, vec![1, 2, 3, 255, 4, 5, 6, 255]);
        assert!(decoder.decode_rgb(&[1, 2], 1, 1).is_err());
        assert!(rgb.get_pixel(5, 0).is_none());

        // Já cabe: devolve cópia
        let same = decoder.resize(&rgb, 10, 10).unwrap();
        assert_eq!(same.rgba_data, rgb.rgba_data);

        // Buffer inconsistente com as dimensões
        let broken = DecodedImage::new(4, 4, vec![0; 4]);
        assert!(decoder.resize(&broken, 2, 2).is_err());
        assert!(broken.get_pixel(1, 1).is_none());

        assert_eq!(resize_to_fit(0, 10, 5, 5), (0, 0));
        assert_eq!(resize_to_fill(0, 10, 5, 5), (0, 0, 0, 0));
    }
}
