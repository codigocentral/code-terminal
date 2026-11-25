use crate::config::{ColorScheme, FontConfig};
use anyhow::Result;
use glyphon::{
    Attrs, Buffer, Cache, Color, Family, FontSystem, Metrics, Shaping, Style, SwashCache, TextArea,
    TextAtlas, TextBounds, TextRenderer as GlyphonTextRenderer, Viewport, Weight,
};
use std::collections::hash_map::DefaultHasher;
use std::hash::{Hash, Hasher};
use std::ops::Range;
use wgpu::{Device, Queue, TextureFormat};

/// Trecho de texto com cor e estilo próprios dentro de uma linha do grid.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct TextSpan {
    pub text: String,
    /// Coluna do grid onde o trecho começa.
    pub col: usize,
    pub color: Color,
    pub bold: bool,
    pub italic: bool,
}

/// Linha do grid já moldada (shaped). A chave evita refazer o shaping
/// de linhas que não mudaram entre frames.
/// Fontes carregadas em segundo plano (ler ~centenas de arquivos de fonte é
/// a parte mais lenta da abertura; começa antes da janela e da GPU).
static PRELOADED_FONTS: std::sync::Mutex<Option<std::thread::JoinHandle<FontSystem>>> =
    std::sync::Mutex::new(None);

/// Começa a carregar as fontes do sistema numa thread (chame no início do `main`).
pub fn preload_fonts() {
    name_worker_pool();
    // No perfil de heap (DHAT) o alocador tem uma trava: o `fork` do PTY
    // com o pool do rayon alocando ao mesmo tempo trava o filho. As fontes
    // são lidas na abertura da janela, depois do PTY.
    if cfg!(feature = "dhat-heap") {
        return;
    }
    if let Ok(mut slot) = PRELOADED_FONTS.lock() {
        if slot.is_none() {
            let spawned = std::thread::Builder::new()
                .name("font-preload".into())
                .spawn(|| {
                    let started = std::time::Instant::now();
                    let fonts = terminal_font_system();
                    tracing::info!(
                        fonts_ms = started.elapsed().as_millis() as u64,
                        faces = fonts.db().len(),
                        "Fonts loaded"
                    );
                    fonts
                });
            // Sem a thread, as fontes são lidas na abertura da janela.
            *slot = spawned.ok();
        }
    }
}

/// Dá nome às threads do pool global do rayon (o cosmic-text lê as fontes
/// em paralelo nele; o wasmtime compila plugins nele). Sem isso elas
/// aparecem com o nome do processo no `/proc/<pid>/task/*/comm` (#30).
fn name_worker_pool() {
    let named = rayon::ThreadPoolBuilder::new()
        .thread_name(|index| format!("pool-{index}"))
        .build_global();
    if let Err(e) = named {
        // Já criado (outra chamada ou um teste): fica como está.
        tracing::debug!(error = %e, "Worker pool already started");
    }
}

/// Banco de fontes já lido, reaproveitado pelas janelas seguintes: clonar
/// custa pouco (os dados das fontes são compartilhados), ler de novo custava
/// dezenas de milissegundos por janela.
static FONT_DB: std::sync::Mutex<Option<(String, glyphon::fontdb::Database)>> =
    std::sync::Mutex::new(None);

fn remember_font_db(fonts: &FontSystem) {
    if let Ok(mut slot) = FONT_DB.lock() {
        if slot.is_none() {
            *slot = Some((fonts.locale().to_string(), fonts.db().clone()));
        }
    }
}

/// Fontes para uma janela: as carregadas em segundo plano, uma cópia das que
/// outra janela já leu, ou (sem nenhuma das duas) lidas agora.
fn take_preloaded_fonts() -> FontSystem {
    let handle = PRELOADED_FONTS.lock().ok().and_then(|mut slot| slot.take());
    if let Some(fonts) = handle.and_then(|handle| handle.join().ok()) {
        remember_font_db(&fonts);
        return fonts;
    }
    if let Some((locale, db)) = FONT_DB.lock().ok().and_then(|slot| slot.clone()) {
        return FontSystem::new_with_locale_and_db(locale, db);
    }
    let fonts = terminal_font_system();
    remember_font_db(&fonts);
    fonts
}

/// Fontes do sistema para o cosmic-text, sem o custo de abrir todas as
/// monoespaçadas. O `FontSystem::new` do cosmic-text 0.12 carrega todas as
/// fontes marcadas como monoespaçadas (93 faces numa máquina comum) para
/// montar o fallback por script: cada uma fica mapeada e com as tabelas de
/// shaping no heap (~10 MB de arquivos e ~3 MB de heap por janela, #1).
/// Aqui só as famílias do terminal (a configurada e o fallback do config)
/// continuam monoespaçadas; as demais seguem no banco para o fallback comum
/// de glifos (ícones, CJK, emoji), carregadas só quando um caractere pede.
fn terminal_font_system() -> FontSystem {
    let font = crate::config::Config::load()
        .map(|config| config.font)
        .unwrap_or_default();
    let mut db = glyphon::fontdb::Database::new();
    db.load_system_fonts();
    let locale = sys_locale::get_locale().unwrap_or_else(|| "en-US".to_string());
    FontSystem::new_with_locale_and_db(locale, only_terminal_fonts_monospaced(db, &font))
}

/// Copia o banco marcando como monoespaçadas só as faces das famílias do
/// terminal (nome sem espaços e sem caixa: "JetBrains Mono" vale para
/// "JetBrainsMono Nerd Font"). As famílias padrão são as do cosmic-text.
fn only_terminal_fonts_monospaced(
    scanned: glyphon::fontdb::Database,
    font: &crate::config::FontConfig,
) -> glyphon::fontdb::Database {
    let normalize = |name: &str| -> String {
        name.chars()
            .filter(|c| c.is_alphanumeric())
            .flat_map(char::to_lowercase)
            .collect()
    };
    let wanted: Vec<String> = std::iter::once(font.family.as_str())
        .chain(font.fallback.iter().map(String::as_str))
        .map(normalize)
        .filter(|name| !name.is_empty())
        .collect();
    let mut db = glyphon::fontdb::Database::new();
    db.set_monospace_family("Fira Mono");
    db.set_sans_serif_family("Fira Sans");
    db.set_serif_family("DejaVu Serif");
    for face in scanned.faces() {
        let mut info = face.clone();
        if info.monospaced {
            info.monospaced = info.families.iter().any(|(name, _)| {
                let name = normalize(name);
                wanted.iter().any(|w| name.contains(w.as_str()))
            });
        }
        db.push_face_info(info);
    }
    db
}

/// Texto de um painel: origem na janela e linhas moldadas.
struct GridText {
    id: u64,
    origin_x: f32,
    origin_y: f32,
    rows: Vec<CachedRow>,
    /// Marcado em `begin_grids` e limpo por `set_pane_lines`: grids que não
    /// foram desenhados no frame são descartados.
    stale: bool,
}

struct CachedRow {
    key: u64,
    /// Trechos moldados e a coluna onde cada um começa.
    segments: Vec<(usize, Buffer)>,
}

/// Parâmetros de fonte usados para moldar uma linha.
struct RowStyle<'a> {
    metrics: Metrics,
    shaping: Shaping,
    family: Family<'a>,
    line_height: f32,
    foreground: Color,
}

/// Molda uma linha: um `Buffer` por segmento, com a coluna onde começa.
fn shape_row(
    font_system: &mut FontSystem,
    style: &RowStyle<'_>,
    spans: &[TextSpan],
) -> Vec<(usize, Buffer)> {
    segment_spans(spans)
        .into_iter()
        .map(|segment| {
            let mut buffer = Buffer::new(font_system, style.metrics);
            buffer.set_size(font_system, None, Some(style.line_height));
            let base = Attrs::new().family(style.family).color(style.foreground);
            buffer.set_rich_text(
                font_system,
                segment.iter().map(|span| {
                    let mut attrs = Attrs::new().family(style.family).color(span.color);
                    if span.bold {
                        attrs = attrs.weight(Weight::BOLD);
                    }
                    if span.italic {
                        attrs = attrs.style(Style::Italic);
                    }
                    (span.text.as_str(), attrs)
                }),
                base,
                style.shaping,
            );
            buffer.shape_until_scroll(font_system, false);
            (segment[0].col, buffer)
        })
        .collect()
}

/// Agrupa os trechos de uma linha em segmentos contíguos. Um segmento novo
/// começa quando há um salto de coluna (caractere largo) ou em caracteres
/// fora da grade da fonte, que são desenhados sozinhos na coluna exata.
fn segment_spans(spans: &[TextSpan]) -> Vec<&[TextSpan]> {
    let fits = |span: &TextSpan| {
        span.text
            .chars()
            .all(crate::renderer::cells::fits_cell_grid)
    };
    let mut segments = Vec::new();
    let mut start = 0;
    for i in 1..spans.len() {
        let prev = &spans[i - 1];
        let contiguous = prev.col + prev.text.chars().count() == spans[i].col;
        if !contiguous || !fits(prev) || !fits(&spans[i]) {
            segments.push(&spans[start..i]);
            start = i;
        }
    }
    if start < spans.len() {
        segments.push(&spans[start..]);
    }
    segments
}

fn line_key(spans: &[TextSpan]) -> u64 {
    let mut hasher = DefaultHasher::new();
    spans.hash(&mut hasher);
    hasher.finish()
}

/// Desenha o texto em duas camadas: o grid do terminal e, por cima de
/// painéis e modais, os textos de UI (overlays). Assim um painel cobre o
/// terminal mas não apaga o próprio texto.
pub struct TextRenderer {
    font_system: FontSystem,
    #[allow(dead_code)]
    cache: Cache,
    swash_cache: SwashCache,
    atlas: TextAtlas,
    grid_renderer: GlyphonTextRenderer,
    overlay_renderer: GlyphonTextRenderer,
    viewport: Viewport,
    /// Um grid por painel visível, com cache de linhas e origem próprios.
    grids: Vec<GridText>,
    /// Grid usado pelas chamadas antigas (`set_lines`/`set_grid_origin`).
    origin_x: f32,
    colors: RenderColors,
    ligatures: bool,
    font_family: String,
    /// Fonte instalada efetivamente usada (None = monospace genérica).
    font_name: Option<String>,
    /// Tamanho da fonte em pixels (usado para calcular cursor e células).
    font_size: f32,
    /// Altura da linha em pixels.
    line_height: f32,
    /// Largura medida de uma célula monoespaçada ('M').
    cell_width: f32,
    offset_y: f32,
    overlays: Vec<TextOverlay>,
    overlay_clip: Option<(Range<usize>, Vec<TextBounds>)>,
    /// Buffers temporários para overlays, mantidos vivos durante render.
    overlay_buffers: Vec<Buffer>,
}

/// Texto overlay a ser desenhado em posição específica da tela.
#[derive(Clone, Debug)]
pub struct TextOverlay {
    pub x: f32,
    pub y: f32,
    pub text: String,
    pub color: Color,
    pub size: f32,
}

#[derive(Clone)]
pub struct RenderColors {
    pub background: Color,
    pub foreground: Color,
    pub cursor: Color,
    pub cursor_background: Color,
    pub selection: Color,
    pub selection_background: Color,
    pub highlight_background: Color,
    pub ansi: [Color; 8],
    pub ansi_bright: [Color; 8],
}

impl Default for RenderColors {
    fn default() -> Self {
        Self {
            background: Color::rgb(0x1e, 0x1e, 0x1e),
            foreground: Color::rgb(0xd4, 0xd4, 0xd4),
            cursor: Color::rgb(0xd4, 0xd4, 0xd4),
            cursor_background: Color::rgb(0xd4, 0xd4, 0xd4),
            selection: Color::rgb(0xd4, 0xd4, 0xd4),
            selection_background: Color::rgb(0x26, 0x4f, 0x78),
            highlight_background: Color::rgb(0xff, 0xff, 0x00),
            ansi: [
                Color::rgb(0x1e, 0x1e, 0x1e), // black
                Color::rgb(0xcd, 0x31, 0x31), // red
                Color::rgb(0x0d, 0xbc, 0x79), // green
                Color::rgb(0xe5, 0xe5, 0x10), // yellow
                Color::rgb(0x24, 0x72, 0xc8), // blue
                Color::rgb(0xbc, 0x3f, 0xbc), // magenta
                Color::rgb(0x11, 0xa8, 0xcd), // cyan
                Color::rgb(0xe5, 0xe5, 0xe5), // white
            ],
            ansi_bright: [
                Color::rgb(0x66, 0x66, 0x66), // bright black
                Color::rgb(0xf1, 0x4c, 0x4c), // bright red
                Color::rgb(0x23, 0xd1, 0x8b), // bright green
                Color::rgb(0xf5, 0xf5, 0x43), // bright yellow
                Color::rgb(0x3b, 0x8e, 0xea), // bright blue
                Color::rgb(0xd6, 0x70, 0xd6), // bright magenta
                Color::rgb(0x29, 0xb8, 0xdb), // bright cyan
                Color::rgb(0xe5, 0xe5, 0xe5), // bright white
            ],
        }
    }
}

fn build_font_family(primary: &str, fallback: &[String]) -> String {
    if fallback.is_empty() {
        return primary.to_string();
    }

    let mut result = primary.to_string();
    for f in fallback {
        result.push_str(", ");
        result.push_str(f);
    }
    result
}

fn parse_hex_color(hex: &str) -> Color {
    let hex = hex.trim_start_matches('#');
    if hex.len() >= 6 {
        let r = u8::from_str_radix(&hex[0..2], 16).unwrap_or(0);
        let g = u8::from_str_radix(&hex[2..4], 16).unwrap_or(0);
        let b = u8::from_str_radix(&hex[4..6], 16).unwrap_or(0);
        Color::rgb(r, g, b)
    } else {
        Color::rgb(0xff, 0xff, 0xff)
    }
}

/// Resolve a família de fonte a ser usada pelo renderer.
/// Garante monoespaçada quando o nome configurado indica uma fonte mono/code.
fn resolve_family(font_family: &str) -> Family<'static> {
    let f = font_family.to_lowercase();
    if f.contains("mono") || f.contains("code") || f.contains("consola") {
        Family::Monospace
    } else if f.contains("serif") {
        Family::Serif
    } else if f.contains("sans") {
        Family::SansSerif
    } else {
        // Padrão seguro para terminal: monoespaçada.
        Family::Monospace
    }
}

/// Primeira fonte da lista (principal + fallbacks) instalada no sistema.
/// Sem isso o renderer usava sempre o "monospace" genérico do fontconfig,
/// ignorando a fonte configurada.
fn pick_installed_font(
    font_system: &FontSystem,
    primary: &str,
    fallback: &[String],
) -> Option<String> {
    let installed: Vec<String> = font_system
        .db()
        .faces()
        .flat_map(|face| face.families.iter().map(|(name, _)| name.clone()))
        .collect();
    choose_font(&installed, primary, fallback)
}

/// Escolhe a fonte: nome exato primeiro; senão uma variante instalada com o
/// mesmo nome sem espaços (ex.: "JetBrains Mono" -> "JetBrainsMono Nerd Font").
fn choose_font(installed: &[String], primary: &str, fallback: &[String]) -> Option<String> {
    let normalize = |name: &str| -> String {
        name.chars()
            .filter(|c| !c.is_whitespace())
            .flat_map(char::to_lowercase)
            .collect()
    };
    let wanted: Vec<&str> = std::iter::once(primary)
        .chain(fallback.iter().map(String::as_str))
        .filter(|name| !name.trim().is_empty())
        .collect();

    for name in &wanted {
        if let Some(found) = installed.iter().find(|i| i.eq_ignore_ascii_case(name)) {
            return Some(found.clone());
        }
        let key = normalize(name);
        let mut variants: Vec<&String> = installed
            .iter()
            .filter(|i| normalize(i).starts_with(&key))
            .collect();
        // Prefere a variante de nome mais curto ("... Nerd Font" a "... Nerd Font Propo").
        variants.sort_by_key(|i| (i.len(), i.as_str()));
        if let Some(found) = variants
            .into_iter()
            .find(|i| !normalize(i).contains("propo"))
        {
            return Some(found.clone());
        }
    }
    None
}

/// Família usada no shaping: a fonte instalada escolhida ou a genérica.
fn family_for<'a>(font_name: &'a Option<String>, font_family: &str) -> Family<'a> {
    match font_name {
        Some(name) => Family::Name(name),
        None => resolve_family(font_family),
    }
}

/// Mede o avanço horizontal do caractere `sample` usando a família resolvida.
/// Para fontes monoespaçadas esse valor é a largura exata de uma célula.
fn measure_cell_width(
    font_system: &mut FontSystem,
    family: Family,
    font_size: f32,
    line_height: f32,
    ligatures: bool,
) -> f32 {
    let metrics = Metrics::new(font_size, line_height);
    let mut buffer = Buffer::new(font_system, metrics);
    buffer.set_size(font_system, Some(f32::MAX), Some(f32::MAX));
    let attrs = Attrs::new().family(family);
    let shaping = if ligatures {
        Shaping::Advanced
    } else {
        Shaping::Basic
    };
    buffer.set_text(font_system, "M", attrs, shaping);
    buffer.shape_until_scroll(font_system, true);

    for line in &buffer.lines {
        if let Some(layout) = line.layout_opt() {
            for layout_line in layout {
                if let Some(glyph) = layout_line.glyphs.first() {
                    return glyph.w;
                }
            }
        }
    }

    // Fallback seguro caso a medição falhe.
    font_size * 0.6
}

impl TextRenderer {
    pub fn new(device: &Device, queue: &Queue, format: TextureFormat) -> Result<Self> {
        Self::new_with_config(device, queue, format, &FontConfig::default())
    }

    pub fn new_with_config(
        device: &Device,
        queue: &Queue,
        format: TextureFormat,
        font_config: &FontConfig,
    ) -> Result<Self> {
        let mut font_system = take_preloaded_fonts();
        let cache = Cache::new(device);
        let swash_cache = SwashCache::new();
        let mut atlas = TextAtlas::new(device, queue, &cache, format);
        let grid_renderer =
            GlyphonTextRenderer::new(&mut atlas, device, wgpu::MultisampleState::default(), None);
        let overlay_renderer =
            GlyphonTextRenderer::new(&mut atlas, device, wgpu::MultisampleState::default(), None);
        let viewport = Viewport::new(device, &cache);

        let font_family = build_font_family(&font_config.family, &font_config.fallback);
        tracing::info!("Using font family: {}", font_family);

        let font_name =
            pick_installed_font(&font_system, &font_config.family, &font_config.fallback);
        tracing::info!(font = ?font_name, "Resolved terminal font");
        let font_size = font_config.size;
        let line_height = (font_config.size * font_config.line_height).ceil();
        let family = family_for(&font_name, &font_family);
        let cell_width = measure_cell_width(
            &mut font_system,
            family,
            font_size,
            line_height,
            font_config.ligatures,
        );

        Ok(Self {
            font_system,
            cache,
            swash_cache,
            atlas,
            grid_renderer,
            overlay_renderer,
            viewport,
            grids: Vec::new(),
            origin_x: 0.0,
            colors: RenderColors::default(),
            ligatures: font_config.ligatures,
            font_family,
            font_name,
            font_size,
            line_height,
            cell_width,
            offset_y: 0.0,
            overlays: Vec::new(),
            overlay_clip: None,
            overlay_buffers: Vec::new(),
        })
    }

    pub fn new_with_color_scheme(
        device: &Device,
        queue: &Queue,
        format: TextureFormat,
        font_config: &FontConfig,
        color_scheme: &ColorScheme,
    ) -> Result<Self> {
        let mut this = Self::new_with_config(device, queue, format, font_config)?;
        this.update_colors(color_scheme);
        Ok(this)
    }

    pub fn update_colors(&mut self, color_scheme: &ColorScheme) {
        self.colors.background = parse_hex_color(&color_scheme.background);
        self.colors.foreground = parse_hex_color(&color_scheme.foreground);
        self.colors.cursor = parse_hex_color(&color_scheme.cursor);
        self.colors.cursor_background = parse_hex_color(&color_scheme.cursor_background);
        self.colors.selection = parse_hex_color(&color_scheme.selection);
        self.colors.selection_background = parse_hex_color(&color_scheme.selection_background);
        self.colors.highlight_background = parse_hex_color(&color_scheme.highlight_background);

        for (i, color) in color_scheme.ansi.iter().enumerate() {
            if i < 8 {
                self.colors.ansi[i] = parse_hex_color(color);
            }
        }

        for (i, color) in color_scheme.ansi_bright.iter().enumerate() {
            if i < 8 {
                self.colors.ansi_bright[i] = parse_hex_color(color);
            }
        }
        // Cores entram no shaping (spans); força refazer as linhas.
        self.grids.clear();
    }

    pub fn get_colors(&self) -> &RenderColors {
        &self.colors
    }

    pub fn update_font(&mut self, font_config: &FontConfig) {
        self.font_size = font_config.size;
        self.line_height = (font_config.size * font_config.line_height).ceil();
        self.ligatures = font_config.ligatures;
        self.font_family = build_font_family(&font_config.family, &font_config.fallback);
        self.font_name = pick_installed_font(
            &self.font_system,
            &font_config.family,
            &font_config.fallback,
        );
        let family = family_for(&self.font_name, &self.font_family);
        self.cell_width = measure_cell_width(
            &mut self.font_system,
            family,
            self.font_size,
            self.line_height,
            font_config.ligatures,
        );
        self.grids.clear();
    }

    pub fn resize(&mut self, width: u32, height: u32, queue: &Queue) {
        self.viewport
            .update(queue, glyphon::Resolution { width, height });
    }

    fn shaping(&self) -> Shaping {
        if self.ligatures {
            Shaping::Advanced
        } else {
            Shaping::Basic
        }
    }

    /// Define as linhas do grid único (compatibilidade: um painel só).
    pub fn set_lines(&mut self, lines: &[Vec<TextSpan>]) {
        self.begin_grids();
        let (x, y) = (self.origin_x, self.offset_y);
        self.set_pane_lines(0, x, y, lines);
        self.end_grids();
    }

    /// Começa um frame: os grids que não receberem linhas serão descartados.
    pub fn begin_grids(&mut self) {
        for grid in &mut self.grids {
            grid.stale = true;
        }
    }

    /// Descarta os grids de painéis que não foram desenhados neste frame.
    pub fn end_grids(&mut self) {
        self.grids.retain(|grid| !grid.stale);
    }

    /// Define as linhas do painel `id`, desenhadas a partir de (x, y). Só as
    /// linhas que mudaram são moldadas de novo.
    pub fn set_pane_lines(&mut self, id: u64, x: f32, y: f32, lines: &[Vec<TextSpan>]) {
        let style = RowStyle {
            metrics: Metrics::new(self.font_size, self.line_height),
            shaping: self.shaping(),
            family: family_for(&self.font_name, &self.font_family),
            line_height: self.line_height,
            foreground: self.colors.foreground,
        };
        let index = match self.grids.iter().position(|grid| grid.id == id) {
            Some(index) => index,
            None => {
                self.grids.push(GridText {
                    id,
                    origin_x: x,
                    origin_y: y,
                    rows: Vec::new(),
                    stale: false,
                });
                self.grids.len() - 1
            }
        };
        let grid = &mut self.grids[index];
        grid.origin_x = x;
        grid.origin_y = y;
        grid.stale = false;
        grid.rows.truncate(lines.len());

        for (i, spans) in lines.iter().enumerate() {
            let key = line_key(spans);
            if grid.rows.get(i).is_some_and(|row| row.key == key) {
                continue;
            }
            let row = CachedRow {
                key,
                segments: shape_row(&mut self.font_system, &style, spans),
            };
            if i < grid.rows.len() {
                grid.rows[i] = row;
            } else {
                grid.rows.push(row);
            }
        }
    }

    /// Define o grid como texto simples, na cor padrão (uma linha por `\n`).
    pub fn set_text(&mut self, text: &str) {
        let foreground = self.colors.foreground;
        let lines: Vec<Vec<TextSpan>> = text
            .lines()
            .map(|line| {
                if line.is_empty() {
                    Vec::new()
                } else {
                    vec![TextSpan {
                        text: line.to_string(),
                        col: 0,
                        color: foreground,
                        bold: false,
                        italic: false,
                    }]
                }
            })
            .collect();
        self.set_lines(&lines);
    }

    /// Retorna o tamanho de uma célula do terminal em pixels.
    /// A largura é medida a partir do glifo 'M' da fonte atual.
    pub fn cell_size(&self) -> (f32, f32) {
        (self.cell_width.max(1.0), self.line_height.max(1.0))
    }

    /// Define o deslocamento vertical do texto em pixels.
    pub fn set_offset_y(&mut self, offset: f32) {
        self.offset_y = offset;
    }

    /// Define onde o grid começa na janela, em pixels.
    pub fn set_grid_origin(&mut self, x: f32, y: f32) {
        self.origin_x = x;
        self.offset_y = y;
    }

    /// Limpa todos os overlays de texto.
    pub fn clear_overlays(&mut self) {
        self.overlays.clear();
        self.overlay_clip = None;
        self.overlay_buffers.clear();
    }

    /// Adiciona um overlay de texto na posição especificada (pixels, origem topo-esquerdo).
    pub fn add_overlay(
        &mut self,
        x: f32,
        y: f32,
        text: impl Into<String>,
        color: Color,
        size: f32,
    ) {
        self.overlays.push(TextOverlay {
            x,
            y,
            text: text.into(),
            color,
            size,
        });
    }

    /// Quantos overlays de texto já foram adicionados neste frame.
    pub fn overlay_count(&self) -> usize {
        self.overlays.len()
    }

    /// Recorta o texto no intervalo `range` onde os painéis o cobrem.
    pub fn clip_overlays_under(&mut self, range: Range<usize>, panels: &[TextBounds]) {
        self.overlay_clip = (!panels.is_empty()).then(|| {
            let viewport = TextBounds {
                left: 0,
                top: 0,
                right: self.viewport.resolution().width as i32,
                bottom: self.viewport.resolution().height as i32,
            };
            (range, visible_overlay_bounds(viewport, panels))
        });
    }

    /// Prepara as duas camadas de texto para o próximo frame.
    pub fn prepare(&mut self, device: &Device, queue: &Queue) -> Result<()> {
        let bounds = TextBounds {
            left: 0,
            top: 0,
            right: self.viewport.resolution().width as i32,
            bottom: self.viewport.resolution().height as i32,
        };

        let shaping = self.shaping();
        let family = family_for(&self.font_name, &self.font_family);
        self.overlay_buffers.clear();
        for overlay in &self.overlays {
            let metrics = Metrics::new(overlay.size, (overlay.size * 1.25).ceil());
            let mut buffer = Buffer::new(&mut self.font_system, metrics);
            buffer.set_size(&mut self.font_system, None, None);
            let attrs = Attrs::new().family(family);
            buffer.set_text(&mut self.font_system, &overlay.text, attrs, shaping);
            buffer.shape_until_scroll(&mut self.font_system, false);
            self.overlay_buffers.push(buffer);
        }

        let cell_width = self.cell_width.max(1.0);
        let line_height = self.line_height;
        let default_color = self.colors.foreground;
        let grid_areas = self.grids.iter().flat_map(|grid| {
            let (origin_x, origin_y) = (grid.origin_x, grid.origin_y);
            grid.rows.iter().enumerate().flat_map(move |(i, row)| {
                let top = origin_y + i as f32 * line_height;
                row.segments.iter().map(move |(col, buffer)| TextArea {
                    buffer,
                    left: origin_x + *col as f32 * cell_width,
                    top,
                    scale: 1.0,
                    bounds,
                    default_color,
                    custom_glyphs: &[],
                })
            })
        });
        self.grid_renderer.prepare(
            device,
            queue,
            &mut self.font_system,
            &mut self.atlas,
            &self.viewport,
            grid_areas,
            &mut self.swash_cache,
        )?;

        let overlay_areas = self
            .overlays
            .iter()
            .zip(&self.overlay_buffers)
            .enumerate()
            .flat_map(|(index, (overlay, buffer))| {
                let regions = self
                    .overlay_clip
                    .as_ref()
                    .filter(|(range, _)| range.contains(&index))
                    .map_or(std::slice::from_ref(&bounds), |(_, regions)| regions);
                regions.iter().map(move |&bounds| TextArea {
                    buffer,
                    left: overlay.x,
                    top: overlay.y,
                    scale: 1.0,
                    bounds,
                    default_color: overlay.color,
                    custom_glyphs: &[],
                })
            });
        self.overlay_renderer.prepare(
            device,
            queue,
            &mut self.font_system,
            &mut self.atlas,
            &self.viewport,
            overlay_areas,
            &mut self.swash_cache,
        )?;
        Ok(())
    }

    /// Desenha o texto do grid (camada de baixo).
    pub fn render_grid(&self, render_pass: &mut wgpu::RenderPass<'_>) -> Result<()> {
        self.grid_renderer
            .render(&self.atlas, &self.viewport, render_pass)?;
        Ok(())
    }

    /// Desenha os textos de UI (camada de cima).
    pub fn render_overlays(&self, render_pass: &mut wgpu::RenderPass<'_>) -> Result<()> {
        self.overlay_renderer
            .render(&self.atlas, &self.viewport, render_pass)?;
        Ok(())
    }

    /// Libera do atlas os glifos que não foram usados no último frame.
    pub fn trim_atlas(&mut self) {
        self.atlas.trim();
    }

    /// Prepara e desenha as duas camadas de uma vez.
    pub fn render(
        &mut self,
        device: &Device,
        queue: &Queue,
        render_pass: &mut wgpu::RenderPass<'_>,
    ) -> Result<()> {
        self.prepare(device, queue)?;
        self.render_grid(render_pass)?;
        self.render_overlays(render_pass)
    }
}

/// Divide a área visível em regiões sem sobreposição para o recorte do Glyphon.
fn visible_overlay_bounds(viewport: TextBounds, panels: &[TextBounds]) -> Vec<TextBounds> {
    let mut visible = vec![viewport];
    for panel in panels {
        visible = visible
            .into_iter()
            .flat_map(|region| {
                let left = region.left.max(panel.left);
                let top = region.top.max(panel.top);
                let right = region.right.min(panel.right);
                let bottom = region.bottom.min(panel.bottom);
                if left >= right || top >= bottom {
                    return [Some(region), None, None, None];
                }
                [
                    (region.top < top).then_some(TextBounds {
                        bottom: top,
                        ..region
                    }),
                    (region.bottom > bottom).then_some(TextBounds {
                        top: bottom,
                        ..region
                    }),
                    (region.left < left).then_some(TextBounds {
                        top,
                        right: left,
                        bottom,
                        ..region
                    }),
                    (region.right > right).then_some(TextBounds {
                        left: right,
                        top,
                        bottom,
                        ..region
                    }),
                ]
            })
            .flatten()
            .collect();
    }
    visible
}

#[cfg(test)]
mod tests {
    #[test]
    fn visible_overlay_bounds_clip_only_pixels_covered_by_panels() {
        let rect = |left, top, right, bottom| TextBounds {
            left,
            top,
            right,
            bottom,
        };
        let viewport = rect(0, 0, 40, 30);
        let panels = [rect(18, 7, 34, 25), rect(25, 18, 45, 32)];
        let visible = visible_overlay_bounds(viewport, &panels);
        let contains = |bounds: &TextBounds, x, y| {
            bounds.left <= x && x < bounds.right && bounds.top <= y && y < bounds.bottom
        };
        for y in viewport.top..viewport.bottom {
            for x in viewport.left..viewport.right {
                let covered = panels.iter().any(|panel| contains(panel, x, y));
                let regions = visible.iter().filter(|bounds| contains(bounds, x, y));
                assert_eq!(regions.count(), usize::from(!covered), "pixel ({x}, {y})");
            }
        }
        assert_eq!(visible_overlay_bounds(viewport, &[]), vec![viewport]);
        assert!(visible_overlay_bounds(viewport, &[viewport]).is_empty());
    }

    use super::*;

    fn face(family: &str, monospaced: bool) -> glyphon::fontdb::FaceInfo {
        use glyphon::fontdb::{FaceInfo, Language, Source, Stretch, Style, Weight, ID};
        FaceInfo {
            id: ID::dummy(),
            source: Source::File(format!("/fontes/{family}.ttf").into()),
            index: 0,
            families: vec![(family.to_string(), Language::English_UnitedStates)],
            post_script_name: family.replace(' ', ""),
            style: Style::Normal,
            weight: Weight::NORMAL,
            stretch: Stretch::Normal,
            monospaced,
        }
    }

    fn mapped_font_files() -> usize {
        let maps = std::fs::read_to_string("/proc/self/maps").unwrap_or_default();
        let mut files: Vec<&str> = maps
            .lines()
            .filter_map(|l| l.split_whitespace().nth(5))
            .filter(|path| path.contains("/fonts/"))
            .collect();
        files.sort_unstable();
        files.dedup();
        files.len()
    }

    fn private_kb() -> u64 {
        std::fs::read_to_string("/proc/self/smaps_rollup")
            .unwrap_or_default()
            .lines()
            .filter(|l| l.starts_with("Private_"))
            .filter_map(|l| l.split_whitespace().nth(1)?.parse::<u64>().ok())
            .sum()
    }

    /// Custo do fallback sob demanda (#1): só as fontes do terminal abrem no
    /// início; a primeira linha com emoji, CJK ou ícone abre a fonte que
    /// tem o glifo. Mede o tempo da primeira moldagem (abre a fonte) contra
    /// a segunda (já aberta), as fontes mapeadas e a memória.
    /// `cargo test --release --lib -- --ignored fallback_cost --nocapture`
    #[test]
    #[ignore = "medição"]
    #[cfg(target_os = "linux")]
    fn fallback_cost() {
        let started = std::time::Instant::now();
        // CT_FONTS_ALL=1: como era antes (o cosmic-text abre todas as
        // monoespaçadas), para comparar no mesmo binário.
        let mut fonts = if std::env::var_os("CT_FONTS_ALL").is_some() {
            FontSystem::new()
        } else {
            terminal_font_system()
        };
        let font = crate::config::FontConfig::default();
        let name = pick_installed_font(&fonts, &font.family, &font.fallback)
            .unwrap_or_else(|| "monospace".into());
        println!(
            "RESULTADO inicio fonte={name} ms={} fontes_mapeadas={} private_kb={}",
            started.elapsed().as_millis(),
            mapped_font_files(),
            private_kb()
        );
        let lines = [
            ("ascii", "The quick brown fox jumps 0123456789"),
            ("acentos", "ação coração pão maçã é ü ñ ç ÁÉÍÓÚ"),
            ("caixa", "┌─┬─┐ ├─┼─┤ └─┴─┘ ╔═╦═╗ ║ ╚═╩═╝ ▀▄█▌▐░▒▓"),
            (
                "powerline",
                "\u{e0b0} \u{e0b1} \u{e0b2} \u{e0a0} \u{e0a2} main",
            ),
            (
                "nerd",
                "\u{f09b} \u{e7a8} \u{f015} \u{f07c} \u{e606} \u{f126}",
            ),
            ("cjk", "中文 日本語 한국어"),
            ("emoji", "🚀 🎉 ✅ ❌ 🔥 👍 🐧"),
        ];
        let attrs = Attrs::new().family(Family::Name(&name));
        for (label, text) in lines {
            let mut times = Vec::new();
            for _ in 0..2 {
                let mut buffer = Buffer::new(&mut fonts, Metrics::new(14.0, 18.0));
                let t = std::time::Instant::now();
                buffer.set_text(&mut fonts, text, attrs, Shaping::Advanced);
                buffer.shape_until_scroll(&mut fonts, false);
                times.push(t.elapsed().as_micros());
            }
            println!(
                "RESULTADO linha={label} primeira_us={} segunda_us={} fontes_mapeadas={} private_kb={}",
                times[0],
                times[1],
                mapped_font_files(),
                private_kb()
            );
        }
    }

    /// Só as famílias do terminal continuam monoespaçadas (o cosmic-text
    /// carrega todas as monoespaçadas ao criar o FontSystem, #1); nenhuma
    /// face sai do banco.
    #[test]
    fn test_only_terminal_families_stay_monospaced() {
        let mut scanned = glyphon::fontdb::Database::new();
        for (family, mono) in [
            ("JetBrainsMono Nerd Font", true),
            ("DejaVu Sans Mono", true),
            ("Adwaita Mono", true),
            ("Liberation Mono", true),
            ("Noto Sans", false),
        ] {
            scanned.push_face_info(face(family, mono));
        }
        let font = crate::config::FontConfig::default();
        let db = only_terminal_fonts_monospaced(scanned, &font);
        let mono: Vec<String> = db
            .faces()
            .filter(|f| f.monospaced)
            .map(|f| f.families[0].0.clone())
            .collect();
        assert_eq!(mono, vec!["JetBrainsMono Nerd Font", "DejaVu Sans Mono"]);
        assert_eq!(db.len(), 5);
        assert_eq!(
            db.family_name(&glyphon::fontdb::Family::Monospace),
            "Fira Mono"
        );
    }

    fn span(text: &str, color: Color) -> TextSpan {
        TextSpan {
            text: text.to_string(),
            col: 0,
            color,
            bold: false,
            italic: false,
        }
    }

    fn span_at(text: &str, col: usize) -> TextSpan {
        TextSpan {
            col,
            ..span(text, Color::rgb(1, 2, 3))
        }
    }

    /// Regressão: emoji/CJK com avanço da fonte de fallback empurravam o resto
    /// da linha para fora das colunas.
    #[test]
    fn test_segment_spans_breaks_on_wide_and_fallback_chars() {
        let spans = vec![
            span_at("ab", 0),
            span_at("c", 2),
            span_at("日", 3),
            span_at("x", 5),
            span_at("✓", 6),
            span_at("yz", 7),
        ];
        let segments = segment_spans(&spans);
        let cols: Vec<usize> = segments.iter().map(|s| s[0].col).collect();
        assert_eq!(cols, vec![0, 3, 5, 6, 7]);
        assert_eq!(segments[0].len(), 2);
        assert!(segment_spans(&[]).is_empty());
    }

    #[test]
    fn test_line_key_changes_with_color_and_text() {
        let red = Color::rgb(255, 0, 0);
        let blue = Color::rgb(0, 0, 255);
        let a = vec![span("abc", red)];
        assert_eq!(line_key(&a), line_key(&a.clone()));
        assert_ne!(line_key(&a), line_key(&[span("abc", blue)]));
        assert_ne!(line_key(&a), line_key(&[span("abd", red)]));
    }

    #[test]
    fn test_choose_font_exact_then_variant_then_fallback() {
        let installed = vec![
            "DejaVu Sans Mono".to_string(),
            "JetBrainsMono Nerd Font Propo".to_string(),
            "JetBrainsMono Nerd Font".to_string(),
        ];
        let fallback = vec!["DejaVu Sans Mono".to_string()];
        assert_eq!(
            choose_font(&installed, "JetBrains Mono", &fallback).as_deref(),
            Some("JetBrainsMono Nerd Font")
        );
        assert_eq!(
            choose_font(&installed, "dejavu sans mono", &[]).as_deref(),
            Some("DejaVu Sans Mono")
        );
        assert_eq!(
            choose_font(&installed, "Fira Code", &fallback).as_deref(),
            Some("DejaVu Sans Mono")
        );
        assert_eq!(choose_font(&installed, "Fira Code", &[]), None);
    }

    #[test]
    fn test_parse_hex_color() {
        assert_eq!(parse_hex_color("#1e1e1e"), Color::rgb(0x1e, 0x1e, 0x1e));
        assert_eq!(parse_hex_color("zz"), Color::rgb(0xff, 0xff, 0xff));
    }

    #[test]
    fn test_resolve_family_defaults_to_monospace() {
        assert_eq!(resolve_family("JetBrains Mono"), Family::Monospace);
        assert_eq!(resolve_family("Whatever"), Family::Monospace);
        assert_eq!(resolve_family("Noto Sans"), Family::SansSerif);
    }
}
