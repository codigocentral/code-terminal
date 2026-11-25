//! Screenshot API - API de Captura de Tela
//!
//! Este módulo fornece funcionalidades para capturar screenshots
//! da página inteira, regiões específicas ou elementos individuais.
//!
//! # Exemplo
//!
//! ```rust,no_run
//! use code_terminal::browser::automation::screenshot::{ScreenshotController, ScreenshotOptions, ScreenshotFormat};
//! use code_terminal::browser::BrowserId;
//!
//! async fn screenshot_example(browser_id: BrowserId) {
//!     let controller = ScreenshotController::new(browser_id);
//!     
//!     // Screenshot da página inteira
//!     let full = controller.capture(ScreenshotOptions::full_page()).await.unwrap();
//!     
//!     // Screenshot de elemento específico
//!     let element = controller.capture(ScreenshotOptions::element("#chart")).await.unwrap();
//!     
//!     // Screenshot de região
//!     let region = controller.capture(ScreenshotOptions::region(0, 0, 800, 600)).await.unwrap();
//! }
//! ```

use crate::browser::{BrowserError, BrowserId, BrowserResult};
use base64::Engine;
use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use std::time::Duration;

/// Controlador de screenshots
#[derive(Debug, Clone)]
pub struct ScreenshotController {
    browser_id: BrowserId,
}

impl ScreenshotController {
    /// Cria um novo controlador
    pub fn new(browser_id: BrowserId) -> Self {
        Self { browser_id }
    }

    /// Retorna o browser associado.
    pub fn browser_id(&self) -> BrowserId {
        self.browser_id
    }

    /// Captura screenshot com opções
    pub async fn capture(&self, options: ScreenshotOptions) -> BrowserResult<ScreenshotResult> {
        tracing::debug!("Capturing screenshot with options: {:?}", options);

        // Simulação de captura
        let dimensions = match &options.target {
            ScreenshotTarget::FullPage => ScreenshotDimensions {
                width: 1920,
                height: 1080,
                device_scale_factor: options.device_scale_factor,
            },
            ScreenshotTarget::Region { width, height, .. } => ScreenshotDimensions {
                width: *width,
                height: *height,
                device_scale_factor: options.device_scale_factor,
            },
            ScreenshotTarget::Element { .. } => ScreenshotDimensions {
                width: 400,
                height: 300,
                device_scale_factor: options.device_scale_factor,
            },
            ScreenshotTarget::Viewport => ScreenshotDimensions {
                width: 1920,
                height: 1080,
                device_scale_factor: options.device_scale_factor,
            },
        };

        // Dados simulados da imagem
        let data = vec![0u8; (dimensions.width * dimensions.height * 4) as usize];

        let result = ScreenshotResult {
            success: true,
            data,
            dimensions,
            format: options.format,
            captured_at: chrono::Utc::now(),
            file_path: options.output_path.clone(),
        };

        // Salva em arquivo se path foi especificado
        if let Some(path) = &options.output_path {
            self.save_to_file(&result.data, path, options.format)
                .await?;
        }

        Ok(result)
    }

    /// Captura screenshot da página inteira
    pub async fn capture_full_page(&self) -> BrowserResult<ScreenshotResult> {
        self.capture(ScreenshotOptions::full_page()).await
    }

    /// Captura screenshot da viewport visível
    pub async fn capture_viewport(&self) -> BrowserResult<ScreenshotResult> {
        self.capture(ScreenshotOptions::viewport()).await
    }

    /// Captura screenshot de elemento
    pub async fn capture_element(
        &self,
        selector: impl Into<String>,
    ) -> BrowserResult<ScreenshotResult> {
        self.capture(ScreenshotOptions::element(selector)).await
    }

    /// Captura screenshot de região
    pub async fn capture_region(
        &self,
        x: u32,
        y: u32,
        width: u32,
        height: u32,
    ) -> BrowserResult<ScreenshotResult> {
        self.capture(ScreenshotOptions::region(x, y, width, height))
            .await
    }

    /// Compara dois screenshots
    pub fn compare(
        &self,
        screenshot1: &ScreenshotResult,
        screenshot2: &ScreenshotResult,
    ) -> ScreenshotComparison {
        if screenshot1.dimensions != screenshot2.dimensions {
            return ScreenshotComparison::DifferentSize;
        }

        // Comparação simples de pixels
        let diff_pixels = screenshot1
            .data
            .iter()
            .zip(screenshot2.data.iter())
            .filter(|(a, b)| a != b)
            .count();

        let total_pixels = screenshot1.data.len();
        let diff_percentage = (diff_pixels as f64 / total_pixels as f64) * 100.0;

        if diff_pixels == 0 {
            ScreenshotComparison::Identical
        } else {
            ScreenshotComparison::Different {
                diff_pixels,
                diff_percentage,
                similarity: 100.0 - diff_percentage,
            }
        }
    }

    /// Salva screenshot em arquivo
    async fn save_to_file(
        &self,
        data: &[u8],
        path: &PathBuf,
        format: ScreenshotFormat,
    ) -> BrowserResult<()> {
        tracing::debug!("Saving screenshot to {:?} as {:?}", path, format);

        // Na implementação real, codificaria e salvaria o arquivo
        tokio::fs::write(path, data)
            .await
            .map_err(|e| BrowserError::Io(format!("Failed to save screenshot: {}", e)))?;

        Ok(())
    }
}

/// Opções de screenshot
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ScreenshotOptions {
    /// Alvo do screenshot
    #[serde(flatten)]
    pub target: ScreenshotTarget,
    /// Formato da imagem
    pub format: ScreenshotFormat,
    /// Qualidade (0-100, para JPEG/WebP)
    pub quality: Option<u8>,
    /// Escala do dispositivo
    pub device_scale_factor: f64,
    /// Se deve capturar fundo transparente
    pub omit_background: bool,
    /// Cor de fundo (se não transparente)
    pub background_color: Option<String>,
    /// Timeout
    pub timeout_ms: u64,
    /// Path para salvar arquivo
    pub output_path: Option<PathBuf>,
}

impl Default for ScreenshotOptions {
    fn default() -> Self {
        Self {
            target: ScreenshotTarget::Viewport,
            format: ScreenshotFormat::Png,
            quality: None,
            device_scale_factor: 1.0,
            omit_background: false,
            background_color: None,
            timeout_ms: 30000,
            output_path: None,
        }
    }
}

impl ScreenshotOptions {
    /// Cria opções para página inteira
    pub fn full_page() -> Self {
        Self {
            target: ScreenshotTarget::FullPage,
            ..Default::default()
        }
    }

    /// Cria opções para viewport
    pub fn viewport() -> Self {
        Self {
            target: ScreenshotTarget::Viewport,
            ..Default::default()
        }
    }

    /// Cria opções para elemento
    pub fn element(selector: impl Into<String>) -> Self {
        Self {
            target: ScreenshotTarget::Element {
                selector: selector.into(),
            },
            ..Default::default()
        }
    }

    /// Cria opções para região
    pub fn region(x: u32, y: u32, width: u32, height: u32) -> Self {
        Self {
            target: ScreenshotTarget::Region {
                x,
                y,
                width,
                height,
            },
            ..Default::default()
        }
    }

    /// Define formato
    pub fn with_format(mut self, format: ScreenshotFormat) -> Self {
        self.format = format;
        self
    }

    /// Define qualidade
    pub fn with_quality(mut self, quality: u8) -> Self {
        self.quality = Some(quality.clamp(0, 100));
        self
    }

    /// Define escala
    pub fn with_scale(mut self, scale: f64) -> Self {
        self.device_scale_factor = scale.clamp(0.1, 5.0);
        self
    }

    /// Define fundo transparente
    pub fn with_transparent_background(mut self) -> Self {
        self.omit_background = true;
        self
    }

    /// Define cor de fundo
    pub fn with_background_color(mut self, color: impl Into<String>) -> Self {
        self.background_color = Some(color.into());
        self.omit_background = false;
        self
    }

    /// Define path de saída
    pub fn save_to(mut self, path: impl Into<PathBuf>) -> Self {
        self.output_path = Some(path.into());
        self
    }

    /// Define timeout
    pub fn with_timeout(mut self, timeout_ms: u64) -> Self {
        self.timeout_ms = timeout_ms;
        self
    }
}

/// Alvo do screenshot
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "target", rename_all = "snake_case")]
pub enum ScreenshotTarget {
    /// Página inteira
    FullPage,
    /// Viewport visível
    Viewport,
    /// Elemento específico
    Element { selector: String },
    /// Região específica
    Region {
        x: u32,
        y: u32,
        width: u32,
        height: u32,
    },
}

/// Formato de imagem
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
#[derive(Default)]
pub enum ScreenshotFormat {
    /// PNG (com transparência)
    #[default]
    Png,
    /// JPEG
    Jpeg,
    /// WebP
    WebP,
    /// BMP
    Bmp,
    /// Raw pixels (RGBA)
    Raw,
}

impl ScreenshotFormat {
    /// Retorna extensão de arquivo
    pub fn extension(&self) -> &'static str {
        match self {
            ScreenshotFormat::Png => "png",
            ScreenshotFormat::Jpeg => "jpg",
            ScreenshotFormat::WebP => "webp",
            ScreenshotFormat::Bmp => "bmp",
            ScreenshotFormat::Raw => "raw",
        }
    }

    /// Retorna MIME type
    pub fn mime_type(&self) -> &'static str {
        match self {
            ScreenshotFormat::Png => "image/png",
            ScreenshotFormat::Jpeg => "image/jpeg",
            ScreenshotFormat::WebP => "image/webp",
            ScreenshotFormat::Bmp => "image/bmp",
            ScreenshotFormat::Raw => "application/octet-stream",
        }
    }

    /// Retorna se suporta transparência
    pub fn supports_alpha(&self) -> bool {
        matches!(
            self,
            ScreenshotFormat::Png | ScreenshotFormat::Raw | ScreenshotFormat::WebP
        )
    }

    /// Retorna se suporta compressão com perda
    pub fn supports_quality(&self) -> bool {
        matches!(self, ScreenshotFormat::Jpeg | ScreenshotFormat::WebP)
    }
}

/// Dimensões do screenshot
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct ScreenshotDimensions {
    /// Largura em pixels
    pub width: u32,
    /// Altura em pixels
    pub height: u32,
    /// Fator de escala do dispositivo
    pub device_scale_factor: f64,
}

impl ScreenshotDimensions {
    /// Retorna número total de pixels
    pub fn total_pixels(&self) -> u64 {
        (self.width as u64) * (self.height as u64)
    }

    /// Retorna tamanho estimado em bytes (RGBA)
    pub fn estimated_size_bytes(&self) -> u64 {
        self.total_pixels() * 4
    }
}

/// Resultado de screenshot
#[derive(Debug, Clone)]
pub struct ScreenshotResult {
    /// Se a captura foi bem-sucedida
    pub success: bool,
    /// Dados da imagem (bytes)
    pub data: Vec<u8>,
    /// Dimensões
    pub dimensions: ScreenshotDimensions,
    /// Formato
    pub format: ScreenshotFormat,
    /// Timestamp
    pub captured_at: chrono::DateTime<chrono::Utc>,
    /// Path do arquivo salvo (se aplicável)
    pub file_path: Option<PathBuf>,
}

impl ScreenshotResult {
    /// Retorna tamanho em KB
    pub fn size_kb(&self) -> f64 {
        self.data.len() as f64 / 1024.0
    }

    /// Salva em arquivo
    pub async fn save(&self, path: impl Into<PathBuf>) -> BrowserResult<()> {
        let path = path.into();
        tokio::fs::write(&path, &self.data)
            .await
            .map_err(|e| BrowserError::Io(format!("Failed to save: {}", e)))?;
        Ok(())
    }

    /// Retorna como base64
    pub fn to_base64(&self) -> String {
        base64::engine::general_purpose::STANDARD.encode(&self.data)
    }

    /// Retorna data URI
    pub fn to_data_uri(&self) -> String {
        format!(
            "data:{};base64,{}",
            self.format.mime_type(),
            self.to_base64()
        )
    }
}

/// Comparação de screenshots
#[derive(Debug, Clone)]
pub enum ScreenshotComparison {
    /// Idênticos
    Identical,
    /// Tamanhos diferentes (não comparável)
    DifferentSize,
    /// Diferentes com estatísticas
    Different {
        /// Pixels diferentes
        diff_pixels: usize,
        /// Porcentagem de diferença
        diff_percentage: f64,
        /// Similaridade (0-100%)
        similarity: f64,
    },
}

impl ScreenshotComparison {
    /// Retorna se são idênticos
    pub fn is_identical(&self) -> bool {
        matches!(self, ScreenshotComparison::Identical)
    }

    /// Retorna similaridade (0-100%)
    pub fn similarity(&self) -> f64 {
        match self {
            ScreenshotComparison::Identical => 100.0,
            ScreenshotComparison::DifferentSize => 0.0,
            ScreenshotComparison::Different { similarity, .. } => *similarity,
        }
    }

    /// Retorna se é similar o suficiente (threshold em %)
    pub fn is_similar(&self, threshold: f64) -> bool {
        self.similarity() >= threshold
    }
}

/// Builder para screenshots em sequência
#[derive(Debug)]
pub struct ScreenshotSequence {
    controller: ScreenshotController,
    screenshots: Vec<ScreenshotResult>,
}

impl ScreenshotSequence {
    /// Cria uma nova sequência
    pub fn new(browser_id: BrowserId) -> Self {
        Self {
            controller: ScreenshotController::new(browser_id),
            screenshots: Vec::new(),
        }
    }

    /// Adiciona screenshot à sequência
    pub async fn capture(
        &mut self,
        options: ScreenshotOptions,
    ) -> BrowserResult<&ScreenshotResult> {
        let result = self.controller.capture(options).await?;
        self.screenshots.push(result);
        Ok(self.screenshots.last().unwrap())
    }

    /// Captura em intervalos regulares
    pub async fn capture_interval(
        &mut self,
        options: ScreenshotOptions,
        interval: Duration,
        count: usize,
    ) -> BrowserResult<()> {
        for _ in 0..count {
            self.capture(options.clone()).await?;
            tokio::time::sleep(interval).await;
        }
        Ok(())
    }

    /// Retorna todos os screenshots
    pub fn screenshots(&self) -> &[ScreenshotResult] {
        &self.screenshots
    }

    /// Cria GIF animado (se suportado)
    pub fn create_gif(&self, _output_path: PathBuf) -> BrowserResult<()> {
        // Na implementação real, criaria um GIF a partir da sequência
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_screenshot_options_default() {
        let opts = ScreenshotOptions::default();
        assert!(matches!(opts.target, ScreenshotTarget::Viewport));
        assert_eq!(opts.format, ScreenshotFormat::Png);
        assert_eq!(opts.device_scale_factor, 1.0);
        assert!(!opts.omit_background);
    }

    #[test]
    fn test_screenshot_options_builders() {
        let opts = ScreenshotOptions::full_page()
            .with_format(ScreenshotFormat::Jpeg)
            .with_quality(90)
            .with_scale(2.0)
            .with_timeout(10000);

        assert!(matches!(opts.target, ScreenshotTarget::FullPage));
        assert_eq!(opts.format, ScreenshotFormat::Jpeg);
        assert_eq!(opts.quality, Some(90));
        assert_eq!(opts.device_scale_factor, 2.0);
        assert_eq!(opts.timeout_ms, 10000);
    }

    #[test]
    fn test_screenshot_format_properties() {
        assert_eq!(ScreenshotFormat::Png.extension(), "png");
        assert_eq!(ScreenshotFormat::Jpeg.mime_type(), "image/jpeg");
        assert!(ScreenshotFormat::Png.supports_alpha());
        assert!(!ScreenshotFormat::Jpeg.supports_alpha());
        assert!(ScreenshotFormat::Jpeg.supports_quality());
        assert!(!ScreenshotFormat::Png.supports_quality());
    }

    #[test]
    fn test_screenshot_dimensions() {
        let dims = ScreenshotDimensions {
            width: 1920,
            height: 1080,
            device_scale_factor: 1.0,
        };

        assert_eq!(dims.total_pixels(), 1920 * 1080);
        assert_eq!(dims.estimated_size_bytes(), 1920 * 1080 * 4);
    }

    #[test]
    fn test_screenshot_comparison() {
        let identical = ScreenshotComparison::Identical;
        assert!(identical.is_identical());
        assert_eq!(identical.similarity(), 100.0);
        assert!(identical.is_similar(95.0));

        let different = ScreenshotComparison::Different {
            diff_pixels: 100,
            diff_percentage: 5.0,
            similarity: 95.0,
        };
        assert!(!different.is_identical());
        assert_eq!(different.similarity(), 95.0);
        assert!(different.is_similar(90.0));
        assert!(!different.is_similar(99.0));
    }

    #[test]
    fn test_screenshot_target_variants() {
        let full = ScreenshotTarget::FullPage;
        let viewport = ScreenshotTarget::Viewport;
        let element = ScreenshotTarget::Element {
            selector: "#test".to_string(),
        };
        let region = ScreenshotTarget::Region {
            x: 0,
            y: 0,
            width: 100,
            height: 100,
        };

        assert!(matches!(full, ScreenshotTarget::FullPage));
        assert!(matches!(viewport, ScreenshotTarget::Viewport));
        assert!(matches!(element, ScreenshotTarget::Element { .. }));
        assert!(matches!(region, ScreenshotTarget::Region { .. }));
    }

    fn result_with(data: Vec<u8>, width: u32, height: u32) -> ScreenshotResult {
        ScreenshotResult {
            success: true,
            data,
            dimensions: ScreenshotDimensions {
                width,
                height,
                device_scale_factor: 1.0,
            },
            format: ScreenshotFormat::Png,
            captured_at: chrono::Utc::now(),
            file_path: None,
        }
    }

    #[tokio::test]
    async fn test_capture_dimensions_per_target() {
        let browser_id = BrowserId::new();
        let controller = ScreenshotController::new(browser_id);
        assert_eq!(controller.browser_id(), browser_id);

        let full = controller.capture_full_page().await.unwrap();
        assert_eq!(
            (full.dimensions.width, full.dimensions.height),
            (1920, 1080)
        );
        assert_eq!(full.data.len(), 1920 * 1080 * 4);
        assert!(full.file_path.is_none());

        let viewport = controller.capture_viewport().await.unwrap();
        assert_eq!(viewport.dimensions.width, 1920);

        let element = controller.capture_element("#chart").await.unwrap();
        assert_eq!(
            (element.dimensions.width, element.dimensions.height),
            (400, 300)
        );

        let region = controller.capture_region(5, 5, 10, 20).await.unwrap();
        assert_eq!(
            (region.dimensions.width, region.dimensions.height),
            (10, 20)
        );
        assert_eq!(region.data.len(), 800);
        assert_eq!(region.format, ScreenshotFormat::Png);

        let scaled = controller
            .capture(
                ScreenshotOptions::region(0, 0, 2, 2)
                    .with_scale(2.0)
                    .with_format(ScreenshotFormat::Jpeg),
            )
            .await
            .unwrap();
        assert_eq!(scaled.dimensions.device_scale_factor, 2.0);
        assert_eq!(scaled.format, ScreenshotFormat::Jpeg);
    }

    #[tokio::test]
    async fn test_capture_saves_to_file() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("shot.png");
        let controller = ScreenshotController::new(BrowserId::new());
        let result = controller
            .capture(ScreenshotOptions::region(0, 0, 3, 2).save_to(&path))
            .await
            .unwrap();
        assert_eq!(result.file_path.as_deref(), Some(path.as_path()));
        assert_eq!(std::fs::read(&path).unwrap().len(), 24);

        // Diretório inexistente: erro de IO
        let bad = dir.path().join("nao/existe.png");
        let err = controller
            .capture(ScreenshotOptions::region(0, 0, 1, 1).save_to(&bad))
            .await
            .unwrap_err();
        assert!(
            err.to_string().contains("Failed to save screenshot"),
            "{err}"
        );
    }

    #[tokio::test]
    async fn test_result_save_and_encodings() {
        let dir = tempfile::tempdir().unwrap();
        let result = result_with(vec![1, 2, 3], 1, 1);
        let path = dir.path().join("r.bin");
        result.save(&path).await.unwrap();
        assert_eq!(std::fs::read(&path).unwrap(), vec![1, 2, 3]);

        let err = result.save(dir.path().join("x/y.bin")).await.unwrap_err();
        assert!(err.to_string().contains("Failed to save"), "{err}");

        assert_eq!(result.to_base64(), "AQID");
        // Regressão: a data URI tinha uma vírgula sobrando no fim
        assert_eq!(result.to_data_uri(), "data:image/png;base64,AQID");
        let kb = result_with(vec![0; 2048], 1, 1).size_kb();
        assert_eq!(kb, 2.0);
    }

    #[test]
    fn test_compare_screenshots() {
        let controller = ScreenshotController::new(BrowserId::new());
        let a = result_with(vec![0, 0, 0, 0], 1, 1);
        let b = result_with(vec![0, 0, 0, 0], 1, 1);
        let c = result_with(vec![0, 255, 0, 0], 1, 1);
        let other_size = result_with(vec![0; 8], 2, 1);

        let same = controller.compare(&a, &b);
        assert!(same.is_identical());
        assert_eq!(same.similarity(), 100.0);

        match controller.compare(&a, &c) {
            ScreenshotComparison::Different {
                diff_pixels,
                diff_percentage,
                similarity,
            } => {
                assert_eq!(diff_pixels, 1);
                assert_eq!(diff_percentage, 25.0);
                assert_eq!(similarity, 75.0);
            }
            other => panic!("esperava Different, veio {other:?}"),
        }
        let diff = controller.compare(&a, &c);
        assert!(diff.is_similar(70.0));
        assert!(!diff.is_similar(80.0));

        let different_size = controller.compare(&a, &other_size);
        assert!(matches!(
            different_size,
            ScreenshotComparison::DifferentSize
        ));
        assert_eq!(different_size.similarity(), 0.0);
        assert!(!different_size.is_identical());
    }

    #[test]
    fn test_options_builders_and_clamping() {
        let opts = ScreenshotOptions::viewport()
            .with_quality(250)
            .with_scale(10.0)
            .with_transparent_background()
            .with_timeout(500);
        assert!(matches!(opts.target, ScreenshotTarget::Viewport));
        assert_eq!(opts.quality, Some(100));
        assert_eq!(opts.device_scale_factor, 5.0);
        assert!(opts.omit_background);
        assert_eq!(opts.timeout_ms, 500);

        // Cor de fundo desliga a transparência
        let opts = opts.with_background_color("#fff");
        assert_eq!(opts.background_color.as_deref(), Some("#fff"));
        assert!(!opts.omit_background);

        assert_eq!(
            ScreenshotOptions::default()
                .with_scale(0.0)
                .device_scale_factor,
            0.1
        );

        // Serialização achata o alvo
        let json = serde_json::to_value(ScreenshotOptions::element("#e")).unwrap();
        assert_eq!(json["target"], "element");
        assert_eq!(json["selector"], "#e");
    }

    #[test]
    fn test_format_properties_all_variants() {
        let cases = [
            (ScreenshotFormat::Png, "png", "image/png", true, false),
            (ScreenshotFormat::Jpeg, "jpg", "image/jpeg", false, true),
            (ScreenshotFormat::WebP, "webp", "image/webp", true, true),
            (ScreenshotFormat::Bmp, "bmp", "image/bmp", false, false),
            (
                ScreenshotFormat::Raw,
                "raw",
                "application/octet-stream",
                true,
                false,
            ),
        ];
        for (format, ext, mime, alpha, quality) in cases {
            assert_eq!(format.extension(), ext);
            assert_eq!(format.mime_type(), mime);
            assert_eq!(format.supports_alpha(), alpha, "{format:?}");
            assert_eq!(format.supports_quality(), quality, "{format:?}");
        }
        assert_eq!(ScreenshotFormat::default(), ScreenshotFormat::Png);
    }

    #[tokio::test]
    async fn test_sequence_collects_screenshots() {
        let mut sequence = ScreenshotSequence::new(BrowserId::new());
        let first = sequence
            .capture(ScreenshotOptions::region(0, 0, 1, 1))
            .await
            .unwrap();
        assert_eq!(first.data.len(), 4);

        sequence
            .capture_interval(
                ScreenshotOptions::region(0, 0, 2, 1),
                Duration::from_millis(1),
                2,
            )
            .await
            .unwrap();
        let widths: Vec<_> = sequence
            .screenshots()
            .iter()
            .map(|s| s.dimensions.width)
            .collect();
        assert_eq!(widths, vec![1, 2, 2]);

        assert!(sequence.create_gif(PathBuf::from("/tmp/x.gif")).is_ok());

        // Erro na captura interrompe a sequência
        let err = sequence
            .capture_interval(
                ScreenshotOptions::region(0, 0, 1, 1).save_to("/nao/existe/dir/x.png"),
                Duration::from_millis(1),
                3,
            )
            .await;
        assert!(err.is_err());
        assert_eq!(sequence.screenshots().len(), 3);
    }
}
