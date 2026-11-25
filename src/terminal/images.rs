//! Imagens inline: protocolo do iTerm2 (`OSC 1337 File=…`) e protocolo
//! gráfico do kitty (`APC G …`).
//!
//! Aqui fica a parte pura: ler os pedidos, decodificar a imagem e calcular
//! quantas células ela ocupa. O grid guarda onde cada imagem foi posta e o
//! renderer desenha a textura por cima das células.

use base64::Engine;
use std::sync::Arc;

/// Limite de bytes de uma imagem (base64) para um programa não esgotar a memória.
pub const MAX_IMAGE_BYTES: usize = 32 * 1024 * 1024;

/// Imagem decodificada (RGBA 8 bits).
pub type Pixels = Arc<image::RgbaImage>;

/// Tamanho pedido para uma dimensão.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SizeSpec {
    Auto,
    Cells(u32),
    Pixels(u32),
    Percent(u32),
}

impl SizeSpec {
    /// `N`, `Npx`, `N%` ou `auto` (iTerm2).
    pub fn parse(value: &str) -> Self {
        let value = value.trim();
        if let Some(px) = value.strip_suffix("px") {
            px.parse().map(Self::Pixels).unwrap_or(Self::Auto)
        } else if let Some(pct) = value.strip_suffix('%') {
            pct.parse().map(Self::Percent).unwrap_or(Self::Auto)
        } else {
            value.parse().map(Self::Cells).unwrap_or(Self::Auto)
        }
    }
}

/// Pedido de imagem pronto para pôr no grid.
#[derive(Clone, Debug)]
pub struct ImageRequest {
    pub pixels: Pixels,
    pub width: SizeSpec,
    pub height: SizeSpec,
    pub preserve_aspect: bool,
}

/// Lê `File=name=…;inline=1;width=…:BASE64` (o que vem depois de `1337;`).
/// Só imagens com `inline=1` são mostradas (sem isso é download de arquivo).
pub fn parse_iterm2(payload: &[u8]) -> Option<ImageRequest> {
    let colon = payload.iter().position(|&b| b == b':')?;
    let (args, data) = (&payload[..colon], &payload[colon + 1..]);
    let args = std::str::from_utf8(args).ok()?.strip_prefix("File=")?;
    if data.len() > MAX_IMAGE_BYTES {
        return None;
    }
    let mut inline = false;
    let mut width = SizeSpec::Auto;
    let mut height = SizeSpec::Auto;
    let mut preserve_aspect = true;
    for arg in args.split(';') {
        let Some((key, value)) = arg.split_once('=') else {
            continue;
        };
        match key {
            "inline" => inline = value == "1",
            "width" => width = SizeSpec::parse(value),
            "height" => height = SizeSpec::parse(value),
            "preserveAspectRatio" => preserve_aspect = value != "0",
            _ => {}
        }
    }
    if !inline {
        return None;
    }
    let bytes = base64::engine::general_purpose::STANDARD
        .decode(strip_whitespace(data))
        .ok()?;
    let pixels = image::load_from_memory(&bytes).ok()?.to_rgba8();
    Some(ImageRequest {
        pixels: Arc::new(pixels),
        width,
        height,
        preserve_aspect,
    })
}

fn strip_whitespace(data: &[u8]) -> Vec<u8> {
    data.iter()
        .copied()
        .filter(|b| !b.is_ascii_whitespace())
        .collect()
}

/// Quantas células (colunas, linhas) a imagem ocupa.
pub fn cells_for(
    image: (u32, u32),
    width: SizeSpec,
    height: SizeSpec,
    preserve_aspect: bool,
    cell: (f32, f32),
    terminal: (usize, usize),
) -> (usize, usize) {
    let (img_w, img_h) = (image.0.max(1) as f32, image.1.max(1) as f32);
    let (cell_w, cell_h) = (cell.0.max(1.0), cell.1.max(1.0));
    let resolve = |spec: SizeSpec, cell: f32, total_cells: usize| -> Option<f32> {
        match spec {
            SizeSpec::Auto => None,
            SizeSpec::Cells(n) => Some(n as f32 * cell),
            SizeSpec::Pixels(px) => Some(px as f32),
            SizeSpec::Percent(p) => Some(total_cells as f32 * cell * p as f32 / 100.0),
        }
    };
    let want_w = resolve(width, cell_w, terminal.0);
    let want_h = resolve(height, cell_h, terminal.1);
    let (mut w, mut h) = match (want_w, want_h) {
        (Some(w), Some(h)) => (w, h),
        (Some(w), None) => (w, img_h * w / img_w),
        (None, Some(h)) => (img_w * h / img_h, h),
        (None, None) => (img_w, img_h),
    };
    if preserve_aspect && want_w.is_some() && want_h.is_some() {
        let scale = (w / img_w).min(h / img_h);
        w = img_w * scale;
        h = img_h * scale;
    }
    // Nunca mais larga que o terminal.
    let max_w = terminal.0.max(1) as f32 * cell_w;
    if w > max_w {
        h *= max_w / w;
        w = max_w;
    }
    let cols = (w / cell_w).ceil().max(1.0) as usize;
    let rows = (h / cell_h).ceil().max(1.0) as usize;
    (cols.min(terminal.0.max(1)), rows)
}

// === Kitty ===

/// Comando do protocolo gráfico do kitty (`ESC _ G chave=valor,… ; dados ESC \`).
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct KittyCommand {
    /// `a`: t (transmitir), T (transmitir e mostrar), p (mostrar), q (consulta), d (apagar)
    pub action: char,
    /// `f`: 24 (RGB), 32 (RGBA) ou 100 (PNG)
    pub format: u32,
    pub id: u32,
    /// `s`, `v`: largura/altura em pixels (formatos crus)
    pub width: u32,
    pub height: u32,
    /// `c`, `r`: colunas/linhas pedidas
    pub cols: u32,
    pub rows: u32,
    /// `m=1`: vem mais pedaço
    pub more: bool,
    /// `q`: 1 cala OK, 2 cala tudo
    pub quiet: u32,
    /// `o=z`: dados comprimidos com zlib (não suportado)
    pub compressed: bool,
    pub payload: Vec<u8>,
}

/// Lê o conteúdo de um APC do kitty (sem o `ESC _` e o `ESC \`).
pub fn parse_kitty(apc: &[u8]) -> Option<KittyCommand> {
    let body = apc.strip_prefix(b"G")?;
    let (control, payload) = match body.iter().position(|&b| b == b';') {
        Some(i) => (&body[..i], &body[i + 1..]),
        None => (body, &b""[..]),
    };
    let mut cmd = KittyCommand {
        action: 't',
        format: 32,
        payload: payload.to_vec(),
        ..Default::default()
    };
    for pair in std::str::from_utf8(control).ok()?.split(',') {
        let Some((key, value)) = pair.split_once('=') else {
            continue;
        };
        let number = || value.parse::<u32>().unwrap_or(0);
        match key {
            "a" => cmd.action = value.chars().next().unwrap_or('t'),
            "f" => cmd.format = number(),
            "i" => cmd.id = number(),
            "s" => cmd.width = number(),
            "v" => cmd.height = number(),
            "c" => cmd.cols = number(),
            "r" => cmd.rows = number(),
            "m" => cmd.more = value == "1",
            "q" => cmd.quiet = number(),
            "o" => cmd.compressed = value == "z",
            _ => {}
        }
    }
    Some(cmd)
}

/// Junta os pedaços (`m=1`) de uma transmissão do kitty.
#[derive(Debug, Default)]
pub struct KittyAssembler {
    pending: Option<KittyCommand>,
}

impl KittyAssembler {
    /// Recebe um comando; devolve o comando completo quando chega o último
    /// pedaço. Os pedaços seguintes só trazem `m` e dados.
    pub fn push(&mut self, cmd: KittyCommand) -> Option<KittyCommand> {
        match self.pending.as_mut() {
            Some(pending) => {
                if pending.payload.len() + cmd.payload.len() > MAX_IMAGE_BYTES {
                    self.pending = None;
                    return None;
                }
                pending.payload.extend_from_slice(&cmd.payload);
                if cmd.more {
                    return None;
                }
                self.pending.take()
            }
            None if cmd.more => {
                self.pending = Some(cmd);
                None
            }
            None => Some(cmd),
        }
    }
}

/// Decodifica os pixels de uma transmissão do kitty.
pub fn decode_kitty(cmd: &KittyCommand) -> Option<image::RgbaImage> {
    if cmd.compressed {
        return None;
    }
    let bytes = base64::engine::general_purpose::STANDARD
        .decode(strip_whitespace(&cmd.payload))
        .ok()?;
    match cmd.format {
        100 => Some(image::load_from_memory(&bytes).ok()?.to_rgba8()),
        32 => image::RgbaImage::from_raw(cmd.width, cmd.height, bytes),
        24 => {
            let rgb = image::RgbImage::from_raw(cmd.width, cmd.height, bytes)?;
            Some(image::DynamicImage::ImageRgb8(rgb).to_rgba8())
        }
        _ => None,
    }
}

/// Resposta do kitty (`ESC _ G i=<id>;OK ESC \`), respeitando o `q`.
pub fn kitty_reply(cmd: &KittyCommand, result: Result<(), &str>) -> Option<Vec<u8>> {
    let message = match result {
        Ok(()) if cmd.quiet >= 1 => return None,
        Err(_) if cmd.quiet >= 2 => return None,
        Ok(()) => "OK".to_string(),
        Err(error) => error.to_string(),
    };
    if cmd.id == 0 {
        return None;
    }
    Some(format!("\x1b_Gi={};{}\x1b\\", cmd.id, message).into_bytes())
}

// === Filtro de APC ===

/// Pedaço da saída do PTY: texto para o parser ou um APC completo.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Segment {
    Text(Vec<u8>),
    Apc(Vec<u8>),
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
enum ApcState {
    #[default]
    Text,
    /// Viu ESC no texto
    Escape,
    /// Dentro de `ESC _ …`
    Apc,
    /// Viu ESC dentro do APC (pode ser o `ESC \` final)
    ApcEscape,
}

/// Separa os APC (`ESC _ … ESC \`) da saída, mantendo a ordem. O vte
/// descarta APC, então as imagens do kitty precisam ser tiradas antes.
#[derive(Debug, Default)]
pub struct ApcFilter {
    state: ApcState,
    apc: Vec<u8>,
}

impl ApcFilter {
    pub fn feed(&mut self, data: &[u8]) -> Vec<Segment> {
        let mut segments = Vec::new();
        let mut text = Vec::with_capacity(data.len());
        for &byte in data {
            match (self.state, byte) {
                (ApcState::Text, 0x1b) => self.state = ApcState::Escape,
                (ApcState::Text, b) => text.push(b),
                (ApcState::Escape, b'_') => {
                    if !text.is_empty() {
                        segments.push(Segment::Text(std::mem::take(&mut text)));
                    }
                    self.state = ApcState::Apc;
                    self.apc.clear();
                }
                (ApcState::Escape, 0x1b) => text.push(0x1b),
                (ApcState::Escape, b) => {
                    text.extend_from_slice(&[0x1b, b]);
                    self.state = ApcState::Text;
                }
                (ApcState::Apc, 0x1b) => self.state = ApcState::ApcEscape,
                (ApcState::Apc, b) => self.push_apc(b),
                (ApcState::ApcEscape, b'\\') => {
                    segments.push(Segment::Apc(std::mem::take(&mut self.apc)));
                    self.state = ApcState::Text;
                }
                (ApcState::ApcEscape, b) => {
                    self.push_apc(0x1b);
                    self.push_apc(b);
                    self.state = ApcState::Apc;
                }
            }
        }
        if !text.is_empty() {
            segments.push(Segment::Text(text));
        }
        segments
    }

    fn push_apc(&mut self, byte: u8) {
        // APC gigante (ou que nunca termina) é descartado.
        if self.apc.len() < MAX_IMAGE_BYTES + 4096 {
            self.apc.push(byte);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// PNG 2x1 (vermelho, azul) em base64.
    fn png_base64() -> String {
        let mut img = image::RgbaImage::new(2, 1);
        img.put_pixel(0, 0, image::Rgba([255, 0, 0, 255]));
        img.put_pixel(1, 0, image::Rgba([0, 0, 255, 255]));
        let mut bytes = Vec::new();
        image::DynamicImage::ImageRgba8(img)
            .write_to(
                &mut std::io::Cursor::new(&mut bytes),
                image::ImageFormat::Png,
            )
            .unwrap();
        base64::engine::general_purpose::STANDARD.encode(bytes)
    }

    #[test]
    fn test_size_spec() {
        assert_eq!(SizeSpec::parse("10"), SizeSpec::Cells(10));
        assert_eq!(SizeSpec::parse("200px"), SizeSpec::Pixels(200));
        assert_eq!(SizeSpec::parse("50%"), SizeSpec::Percent(50));
        assert_eq!(SizeSpec::parse("auto"), SizeSpec::Auto);
    }

    #[test]
    fn test_parse_iterm2_inline_image() {
        let payload = format!("File=name=eA==;width=4;inline=1:{}", png_base64());
        let request = parse_iterm2(payload.as_bytes()).unwrap();
        assert_eq!(request.pixels.dimensions(), (2, 1));
        assert_eq!(
            *request.pixels.get_pixel(1, 0),
            image::Rgba([0, 0, 255, 255])
        );
        assert_eq!(request.width, SizeSpec::Cells(4));
        // Sem inline=1 é download, não imagem.
        let download = format!("File=name=eA==:{}", png_base64());
        assert!(parse_iterm2(download.as_bytes()).is_none());
        assert!(parse_iterm2(b"File=inline=1:!!notbase64").is_none());
    }

    #[test]
    fn test_cells_for() {
        let cell = (10.0, 20.0);
        let term = (80, 24);
        // Tamanho natural: 100x40 px -> 10 colunas, 2 linhas.
        assert_eq!(
            cells_for((100, 40), SizeSpec::Auto, SizeSpec::Auto, true, cell, term),
            (10, 2)
        );
        // Largura em células mantém a proporção.
        assert_eq!(
            cells_for(
                (100, 40),
                SizeSpec::Cells(20),
                SizeSpec::Auto,
                true,
                cell,
                term
            ),
            (20, 4)
        );
        // Nunca passa da largura do terminal.
        assert_eq!(
            cells_for(
                (5000, 100),
                SizeSpec::Auto,
                SizeSpec::Auto,
                true,
                cell,
                term
            )
            .0,
            80
        );
        // Caixa com proporção preservada.
        let (cols, rows) = cells_for(
            (100, 100),
            SizeSpec::Cells(40),
            SizeSpec::Cells(5),
            true,
            cell,
            term,
        );
        assert_eq!((cols, rows), (10, 5));
        assert_eq!(
            cells_for(
                (100, 100),
                SizeSpec::Percent(50),
                SizeSpec::Auto,
                true,
                cell,
                term
            )
            .0,
            40
        );
    }

    #[test]
    fn test_parse_and_assemble_kitty_chunks() {
        let data = png_base64();
        let (a, b) = data.split_at(data.len() / 2);
        let first = parse_kitty(format!("Ga=T,f=100,i=7,c=3,m=1;{a}").as_bytes()).unwrap();
        assert_eq!(
            (first.action, first.format, first.id, first.cols),
            ('T', 100, 7, 3)
        );
        let mut assembler = KittyAssembler::default();
        assert!(assembler.push(first).is_none());
        let last = parse_kitty(format!("Gm=0;{b}").as_bytes()).unwrap();
        let full = assembler.push(last).unwrap();
        assert_eq!(full.id, 7);
        assert_eq!(decode_kitty(&full).unwrap().dimensions(), (2, 1));
        assert!(parse_kitty(b"Xnot-graphics").is_none());
    }

    #[test]
    fn test_decode_kitty_raw_formats_and_reply() {
        let rgba = base64::engine::general_purpose::STANDARD.encode([1u8, 2, 3, 4, 5, 6, 7, 8]);
        let cmd = parse_kitty(format!("Gf=32,s=2,v=1,i=1;{rgba}").as_bytes()).unwrap();
        assert_eq!(decode_kitty(&cmd).unwrap().get_pixel(1, 0).0, [5, 6, 7, 8]);
        let rgb = base64::engine::general_purpose::STANDARD.encode([9u8, 9, 9]);
        let cmd = parse_kitty(format!("Gf=24,s=1,v=1;{rgb}").as_bytes()).unwrap();
        assert_eq!(
            decode_kitty(&cmd).unwrap().get_pixel(0, 0).0,
            [9, 9, 9, 255]
        );
        let wrong = parse_kitty(b"Gf=32,s=9,v=9;AAAA").unwrap();
        assert!(decode_kitty(&wrong).is_none());

        let query = parse_kitty(b"Ga=q,i=31,s=1,v=1,f=24;AAAA").unwrap();
        assert_eq!(kitty_reply(&query, Ok(())).unwrap(), b"\x1b_Gi=31;OK\x1b\\");
        let quiet = parse_kitty(b"Ga=T,i=2,q=1;").unwrap();
        assert_eq!(kitty_reply(&quiet, Ok(())), None);
        assert!(kitty_reply(&quiet, Err("ENODATA")).is_some());
    }

    #[test]
    fn test_apc_filter_splits_in_order_across_chunks() {
        let mut filter = ApcFilter::default();
        let mut segments = filter.feed(b"abc\x1b_Ga=q;AA");
        segments.extend(filter.feed(b"AA\x1b\\def\x1b[31mx"));
        assert_eq!(
            segments,
            vec![
                Segment::Text(b"abc".to_vec()),
                Segment::Apc(b"Ga=q;AAAA".to_vec()),
                Segment::Text(b"def\x1b[31mx".to_vec()),
            ]
        );
        // ESC solto no fim de um pedaço continua com o próximo.
        let mut segments = filter.feed(b"x\x1b");
        segments.extend(filter.feed(b"[0m"));
        let text: Vec<u8> = segments
            .into_iter()
            .flat_map(|s| match s {
                Segment::Text(t) => t,
                Segment::Apc(_) => Vec::new(),
            })
            .collect();
        assert_eq!(text, b"x\x1b[0m");
    }
}
