//! Animação da tab com aviso (#26): estilo, velocidade e "menos movimento".
//!
//! A animação é calculada a cada quadro a partir da idade do aviso, sem
//! quadros guardados. Só os estilos que se mexem acordam o render (por
//! [`crate::ui::glow::GLOW_DURATION`] depois de um aviso); "Só um ponto" e
//! "Nenhuma" deixam o app dormindo.

use serde::{Deserialize, Serialize};
use std::sync::atomic::{AtomicBool, AtomicU8, Ordering};
use std::time::Duration;

/// Estilo da animação do aviso.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum TabAnimation {
    /// Anel colorido: as cores passeiam e o anel pulsa (o de sempre).
    #[default]
    Ring,
    /// Pulso: uma cor só, acendendo e apagando.
    Pulse,
    /// Onda de cor: as cores descem pela faixa da tab.
    Wave,
    /// Indicador giratório ao lado do título.
    Spinner,
    /// Só um ponto, parado.
    Dot,
    /// Nada se mexe.
    None,
}

impl TabAnimation {
    /// Todos, na ordem do seletor.
    pub const ALL: [TabAnimation; 6] = [
        TabAnimation::Ring,
        TabAnimation::Pulse,
        TabAnimation::Wave,
        TabAnimation::Spinner,
        TabAnimation::Dot,
        TabAnimation::None,
    ];

    /// Se o estilo se mexe (e precisa acordar o render).
    pub fn moves(self) -> bool {
        !matches!(self, TabAnimation::Dot | TabAnimation::None)
    }

    /// Nome no seletor.
    pub fn label(self) -> &'static str {
        use crate::ui::i18n::{tr, Text};
        tr(match self {
            TabAnimation::Ring => Text::AnimationRing,
            TabAnimation::Pulse => Text::AnimationPulse,
            TabAnimation::Wave => Text::AnimationWave,
            TabAnimation::Spinner => Text::AnimationSpinner,
            TabAnimation::Dot => Text::AnimationDot,
            TabAnimation::None => Text::AnimationNone,
        })
    }
}

/// Velocidade da animação.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum AnimationSpeed {
    Slow,
    #[default]
    Normal,
    Fast,
}

impl AnimationSpeed {
    pub const ALL: [AnimationSpeed; 3] = [
        AnimationSpeed::Slow,
        AnimationSpeed::Normal,
        AnimationSpeed::Fast,
    ];

    /// Quanto o tempo anda para a animação (lenta = metade).
    pub fn factor(self) -> f32 {
        match self {
            AnimationSpeed::Slow => 0.5,
            AnimationSpeed::Normal => 1.0,
            AnimationSpeed::Fast => 2.0,
        }
    }

    pub fn label(self) -> &'static str {
        use crate::ui::i18n::{tr, Text};
        tr(match self {
            AnimationSpeed::Slow => Text::SpeedSlow,
            AnimationSpeed::Normal => Text::SpeedNormal,
            AnimationSpeed::Fast => Text::SpeedFast,
        })
    }
}

/// O que o render usa: estilo e velocidade já resolvidos.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Motion {
    pub style: TabAnimation,
    pub speed: AnimationSpeed,
}

impl Motion {
    /// Parado (sem animação).
    pub const STILL: Motion = Motion {
        style: TabAnimation::None,
        speed: AnimationSpeed::Normal,
    };

    /// Resolve a configuração: `animations = false` ou o sistema pedindo
    /// menos movimento (quando a opção de seguir o sistema está ligada)
    /// deixam tudo parado; "Só um ponto" continua mostrando o ponto.
    pub fn resolve(
        animations: bool,
        style: TabAnimation,
        speed: AnimationSpeed,
        follow_system: bool,
        system_reduced: bool,
    ) -> Motion {
        let still = !animations || (follow_system && system_reduced);
        let style = if still && style.moves() {
            TabAnimation::None
        } else {
            style
        };
        Motion { style, speed }
    }

    /// Idade do aviso no relógio da animação (a velocidade estica ou encolhe).
    pub fn scaled(self, age: Duration) -> Duration {
        age.mul_f32(self.speed.factor())
    }
}

/// Quadros do indicador giratório.
pub const SPINNER_FRAMES: [&str; 10] = [
    "\u{280b}", "\u{2819}", "\u{2839}", "\u{2838}", "\u{283c}", "\u{2834}", "\u{2826}", "\u{2827}",
    "\u{2807}", "\u{280f}",
];

/// Tempo de cada quadro do indicador na velocidade normal.
const SPINNER_STEP: Duration = Duration::from_millis(100);

/// Quadro do indicador para o instante `elapsed`.
pub fn spinner_frame(elapsed: Duration, speed: AnimationSpeed) -> &'static str {
    let step = SPINNER_STEP.as_secs_f32() / speed.factor();
    let index = (elapsed.as_secs_f32() / step) as usize % SPINNER_FRAMES.len();
    SPINNER_FRAMES[index]
}

/// Símbolos que programas usam como indicador no começo do título
/// (Claude Code: ◐◓◑◒ e ✳✶✻✽; spinners de CLI: braille ⠋⠙⠹…).
fn is_program_spinner(c: char) -> bool {
    matches!(
        c,
        '\u{25d0}'
            ..='\u{25d3}'
                | '\u{2733}'
                | '\u{2736}'
                | '\u{273b}'
                | '\u{273d}'
                | '\u{2722}'
                | '\u{00b7}'
    ) || ('\u{2800}'..='\u{28ff}').contains(&c)
}

/// Título sem o indicador do programa no começo, se houver um.
pub fn strip_program_spinner(title: &str) -> Option<&str> {
    let mut chars = title.chars();
    let first = chars.next()?;
    if !is_program_spinner(first) {
        return None;
    }
    let rest = chars.as_str().trim_start();
    (!rest.is_empty()).then_some(rest)
}

/// Resposta guardada: 0 = ainda não lida, 1 = não, 2 = sim.
static REDUCED_MOTION: AtomicU8 = AtomicU8::new(0);
/// Uma releitura em segundo plano já está rodando.
static REFRESHING: AtomicBool = AtomicBool::new(false);

/// O sistema pede menos movimento? (GNOME: `enable-animations`; macOS:
/// "Reduzir movimento"; Windows: "Efeitos de animação".)
/// `CODE_TERMINAL_REDUCED_MOTION=1` força. Lido na primeira vez e de novo a
/// cada vez que a janela ganha foco ([`refresh_reduced_motion`]), então
/// mudar a opção do sistema com o app aberto vale ao voltar para ele.
pub fn system_prefers_reduced_motion() -> bool {
    match REDUCED_MOTION.load(Ordering::Relaxed) {
        0 => {
            let reduced = detect_reduced_motion();
            store_reduced_motion(reduced);
            reduced
        }
        value => value == 2,
    }
}

fn store_reduced_motion(reduced: bool) {
    REDUCED_MOTION.store(if reduced { 2 } else { 1 }, Ordering::Relaxed);
}

/// Relê a opção do sistema numa thread curta (o `gsettings`/`defaults`
/// demora alguns ms e não pode travar a janela). Chamado quando a janela
/// ganha foco: sem thread nem timer com o app parado. Devolve se começou
/// uma releitura (não começa se outra ainda está rodando).
pub fn refresh_reduced_motion() -> bool {
    if REFRESHING.swap(true, Ordering::AcqRel) {
        return false;
    }
    let spawned = std::thread::Builder::new()
        .name("reduced-motion".into())
        .spawn(|| {
            store_reduced_motion(detect_reduced_motion());
            REFRESHING.store(false, Ordering::Release);
        });
    if spawned.is_err() {
        REFRESHING.store(false, Ordering::Release);
        return false;
    }
    true
}

fn detect_reduced_motion() -> bool {
    if let Ok(value) = std::env::var("CODE_TERMINAL_REDUCED_MOTION") {
        return matches!(value.trim(), "1" | "true" | "yes");
    }
    // Nos testes a resposta não depende da máquina.
    if cfg!(test) {
        return false;
    }
    let output = |program: &str, args: &[&str]| {
        std::process::Command::new(program)
            .args(args)
            .stderr(std::process::Stdio::null())
            .output()
            .ok()
            .filter(|out| out.status.success())
            .map(|out| String::from_utf8_lossy(&out.stdout).trim().to_string())
    };
    if cfg!(target_os = "macos") {
        return output(
            "defaults",
            &["read", "com.apple.universalaccess", "reduceMotion"],
        )
        .is_some_and(|value| value == "1");
    }
    if cfg!(all(unix, not(target_os = "macos"))) {
        return output(
            "gsettings",
            &["get", "org.gnome.desktop.interface", "enable-animations"],
        )
        .is_some_and(|value| value == "false");
    }
    windows_animations_disabled()
}

/// Windows: "Efeitos de animação" desligado (Configurações › Acessibilidade
/// › Efeitos visuais), lido com `SPI_GETCLIENTAREAANIMATION`.
#[cfg(windows)]
fn windows_animations_disabled() -> bool {
    const SPI_GETCLIENTAREAANIMATION: u32 = 0x1042;
    #[link(name = "user32")]
    extern "system" {
        fn SystemParametersInfoW(
            action: u32,
            param: u32,
            value: *mut core::ffi::c_void,
            ini: u32,
        ) -> i32;
    }
    let mut enabled: i32 = 1;
    // SAFETY: SPI_GETCLIENTAREAANIMATION escreve um BOOL (i32) no ponteiro,
    // que aponta para `enabled`, vivo durante a chamada.
    let ok = unsafe {
        SystemParametersInfoW(
            SPI_GETCLIENTAREAANIMATION,
            0,
            (&mut enabled as *mut i32).cast(),
            0,
        )
    };
    ok != 0 && enabled == 0
}

#[cfg(not(windows))]
fn windows_animations_disabled() -> bool {
    false
}

#[cfg(test)]
mod tests {
    use super::*;

    /// #32: a releitura roda em segundo plano, grava o valor e libera a
    /// próxima (sem empilhar threads). Só grava "não": outros testes leem o
    /// mesmo valor global ao mesmo tempo.
    #[test]
    fn refresh_rereads_in_the_background() {
        if std::env::var_os("CODE_TERMINAL_REDUCED_MOTION").is_some() {
            return;
        }
        store_reduced_motion(false);
        let deadline = std::time::Instant::now() + Duration::from_secs(5);
        while !refresh_reduced_motion() && std::time::Instant::now() < deadline {
            std::thread::sleep(Duration::from_millis(5));
        }
        while REFRESHING.load(Ordering::Acquire) && std::time::Instant::now() < deadline {
            std::thread::sleep(Duration::from_millis(5));
        }
        assert!(!REFRESHING.load(Ordering::Acquire));
        assert_eq!(REDUCED_MOTION.load(Ordering::Relaxed), 1);
        assert!(!system_prefers_reduced_motion());
    }

    #[test]
    fn only_moving_styles_wake_the_renderer() {
        for style in TabAnimation::ALL {
            assert_eq!(
                style.moves(),
                !matches!(style, TabAnimation::Dot | TabAnimation::None),
                "{style:?}"
            );
            assert!(!style.label().is_empty());
        }
    }

    /// Respeita "menos movimento": com a opção ligada e o sistema pedindo,
    /// nada se mexe; o ponto parado continua.
    #[test]
    fn reduced_motion_and_disabled_animations_stop_everything() {
        use AnimationSpeed::Normal;
        let resolve = |animations, style, follow, reduced| {
            Motion::resolve(animations, style, Normal, follow, reduced).style
        };
        assert_eq!(
            resolve(true, TabAnimation::Pulse, true, false),
            TabAnimation::Pulse
        );
        assert_eq!(
            resolve(true, TabAnimation::Pulse, true, true),
            TabAnimation::None
        );
        assert_eq!(
            resolve(true, TabAnimation::Pulse, false, true),
            TabAnimation::Pulse
        );
        assert_eq!(
            resolve(false, TabAnimation::Wave, false, false),
            TabAnimation::None
        );
        assert_eq!(
            resolve(true, TabAnimation::Dot, true, true),
            TabAnimation::Dot
        );
    }

    #[test]
    fn speed_scales_the_clock() {
        let motion = |speed| Motion {
            style: TabAnimation::Ring,
            speed,
        };
        let age = Duration::from_secs(2);
        assert_eq!(
            motion(AnimationSpeed::Slow).scaled(age),
            Duration::from_secs(1)
        );
        assert_eq!(
            motion(AnimationSpeed::Fast).scaled(age),
            Duration::from_secs(4)
        );
        assert_eq!(
            spinner_frame(Duration::ZERO, AnimationSpeed::Normal),
            SPINNER_FRAMES[0]
        );
        assert_eq!(
            spinner_frame(Duration::from_millis(150), AnimationSpeed::Normal),
            SPINNER_FRAMES[1]
        );
        assert_eq!(
            spinner_frame(Duration::from_millis(150), AnimationSpeed::Fast),
            SPINNER_FRAMES[3]
        );
    }

    #[test]
    fn strips_known_program_spinners_only() {
        assert_eq!(
            strip_program_spinner("\u{25d0} Claude Code"),
            Some("Claude Code")
        );
        assert_eq!(strip_program_spinner("\u{2733} Working"), Some("Working"));
        assert_eq!(
            strip_program_spinner("\u{2819} npm install"),
            Some("npm install")
        );
        assert_eq!(strip_program_spinner("vim main.rs"), None);
        assert_eq!(strip_program_spinner("\u{25d0}"), None, "só o símbolo fica");
        assert_eq!(strip_program_spinner(""), None);
    }

    #[test]
    fn config_names_are_kebab_case() {
        #[derive(Serialize, Deserialize)]
        struct Wrap {
            style: TabAnimation,
            speed: AnimationSpeed,
        }
        let text = toml::to_string(&Wrap {
            style: TabAnimation::Spinner,
            speed: AnimationSpeed::Fast,
        })
        .unwrap();
        assert!(text.contains("style = \"spinner\"") && text.contains("speed = \"fast\""));
        let back: Wrap = toml::from_str("style = \"none\"\nspeed = \"slow\"").unwrap();
        assert_eq!(back.style, TabAnimation::None);
        assert_eq!(back.speed, AnimationSpeed::Slow);
    }
}
