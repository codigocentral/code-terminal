//! Cores animadas do aviso: o anel em volta do painel, a faixa e o contador
//! da tab passeiam pelas cores de destaque do tema por alguns segundos depois
//! que um aviso chega, e então param na primeira cor.
//!
//! O app dorme quando nada acontece. A animação não muda isso: um relógio
//! acorda a janela a ~30 quadros por segundo só enquanto há aviso recente e
//! para sozinho depois de [`GLOW_DURATION`].

use glyphon::Color;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, LazyLock};
use std::time::{Duration, Instant};

/// Quanto tempo as cores passeiam depois de um aviso.
pub const GLOW_DURATION: Duration = Duration::from_secs(8);

/// Tempo de uma volta completa pelas cores de destaque.
const CYCLE: Duration = Duration::from_millis(2400);

/// Intervalo entre quadros enquanto anima (~30 fps).
const FRAME: Duration = Duration::from_millis(33);

/// A animação ainda está rodando para um aviso com essa idade?
pub fn is_animating(age: Duration, enabled: bool) -> bool {
    enabled && age < GLOW_DURATION
}

/// Cor do aviso com `age` de idade. Durante [`GLOW_DURATION`] passeia por
/// `accents` em ordem, com transição suave; depois (ou com animações
/// desligadas) fica em `accents[0]`.
pub fn glow_color(accents: &[Color], age: Duration, enabled: bool) -> Color {
    let Some(&first) = accents.first() else {
        return Color::rgb(255, 255, 255);
    };
    if accents.len() < 2 || !is_animating(age, enabled) {
        return first;
    }
    cycle_color(accents, age)
}

/// Cor do passeio pelas cores de destaque no instante `t` (sem prazo).
fn cycle_color(accents: &[Color], t: Duration) -> Color {
    let Some(&first) = accents.first() else {
        return Color::rgb(255, 255, 255);
    };
    if accents.len() < 2 {
        return first;
    }
    let n = accents.len();
    let turns = t.as_secs_f32() / CYCLE.as_secs_f32();
    let position = turns.fract() * n as f32;
    let index = position.floor() as usize % n;
    let t = smoothstep(position.fract());
    lerp(accents[index], accents[(index + 1) % n], t)
}

/// Espessura do anel: pulsa entre 2 e 4 px enquanto anima, 2 px parado.
pub fn ring_width(age: Duration, enabled: bool) -> f32 {
    if !is_animating(age, enabled) {
        return 2.0;
    }
    let phase = age.as_secs_f32() / CYCLE.as_secs_f32() * std::f32::consts::TAU * 2.0;
    3.0 + phase.sin()
}

use crate::ui::tab_animation::{Motion, TabAnimation};

/// A animação do aviso com `age` de idade ainda se mexe neste estilo?
pub fn is_notice_animating(age: Duration, motion: Motion) -> bool {
    motion.style.moves() && age < GLOW_DURATION
}

/// Cor do aviso no estilo escolhido (#26). `offset` (0 a 1) adianta a onda
/// ao longo da faixa no estilo "Onda de cor"; os outros ignoram.
pub fn notice_color(accents: &[Color], age: Duration, motion: Motion, offset: f32) -> Color {
    let Some(&first) = accents.first() else {
        return Color::rgb(255, 255, 255);
    };
    if !is_notice_animating(age, motion) {
        return first;
    }
    let t = motion.scaled(age);
    match motion.style {
        TabAnimation::Ring => cycle_color(accents, t),
        TabAnimation::Wave => cycle_color(accents, t + CYCLE.mul_f32(offset.clamp(0.0, 1.0))),
        TabAnimation::Pulse => {
            let phase = (t.as_secs_f32() / 1.2 * std::f32::consts::TAU).sin() * 0.5 + 0.5;
            lerp(
                lerp(first, Color::rgb(0, 0, 0), 0.6),
                first,
                smoothstep(phase),
            )
        }
        // O indicador gira ao lado do título; a cor fica parada.
        TabAnimation::Spinner | TabAnimation::Dot | TabAnimation::None => first,
    }
}

/// Espessura do anel do painel no estilo escolhido: só o "Anel colorido"
/// pulsa.
pub fn notice_ring_width(age: Duration, motion: Motion) -> f32 {
    if motion.style == TabAnimation::Ring && is_notice_animating(age, motion) {
        let phase =
            motion.scaled(age).as_secs_f32() / CYCLE.as_secs_f32() * std::f32::consts::TAU * 2.0;
        3.0 + phase.sin()
    } else {
        2.0
    }
}

fn smoothstep(t: f32) -> f32 {
    let t = t.clamp(0.0, 1.0);
    t * t * (3.0 - 2.0 * t)
}

fn lerp(a: Color, b: Color, t: f32) -> Color {
    let mix = |x: u8, y: u8| (f32::from(x) + (f32::from(y) - f32::from(x)) * t).round() as u8;
    Color::rgba(
        mix(a.r(), b.r()),
        mix(a.g(), b.g()),
        mix(a.b(), b.b()),
        mix(a.a(), b.a()),
    )
}

/// Relógio que acorda a janela enquanto há animação.
struct WakeClock {
    epoch: Instant,
    /// Até quando (ms desde `epoch`) acordar a janela.
    until_ms: AtomicU64,
    running: AtomicBool,
    /// O que fazer a cada quadro (no app, pedir um redesenho).
    tick: fn(),
}

impl WakeClock {
    fn new(tick: fn()) -> Arc<Self> {
        Arc::new(Self {
            epoch: Instant::now(),
            until_ms: AtomicU64::new(0),
            running: AtomicBool::new(false),
            tick,
        })
    }

    fn now_ms(&self) -> u64 {
        u64::try_from(self.epoch.elapsed().as_millis()).unwrap_or(u64::MAX)
    }

    fn keep_awake(self: &Arc<Self>, duration: Duration) {
        let extra = u64::try_from(duration.as_millis()).unwrap_or(u64::MAX);
        self.until_ms
            .fetch_max(self.now_ms().saturating_add(extra), Ordering::AcqRel);
        if self.running.swap(true, Ordering::AcqRel) {
            return;
        }
        let clock = Arc::clone(self);
        let spawned = std::thread::Builder::new()
            .name("glow-clock".into())
            .spawn(move || clock.run());
        if let Err(error) = spawned {
            self.running.store(false, Ordering::Release);
            tracing::warn!(error = %error, "Failed to start the animation clock");
        }
    }

    fn run(&self) {
        loop {
            std::thread::sleep(FRAME);
            (self.tick)();
            if self.now_ms() < self.until_ms.load(Ordering::Acquire) {
                continue;
            }
            self.running.store(false, Ordering::Release);
            // Um aviso pode ter estendido o prazo entre a leitura e o store:
            // se ninguém assumiu o relógio, este continua.
            if self.now_ms() < self.until_ms.load(Ordering::Acquire)
                && !self.running.swap(true, Ordering::AcqRel)
            {
                continue;
            }
            break;
        }
        // Um último quadro, já com a cor parada.
        (self.tick)();
    }

    #[cfg(test)]
    fn is_running(&self) -> bool {
        self.running.load(Ordering::Acquire)
    }
}

static CLOCK: LazyLock<Arc<WakeClock>> =
    LazyLock::new(|| WakeClock::new(crate::terminal::pty::request_redraw));

/// Mantém a janela acordando por `duration` (a animação de um aviso novo).
/// Chamadas seguidas só estendem o prazo; nunca há mais de um relógio.
pub fn keep_awake(duration: Duration) {
    CLOCK.keep_awake(duration);
}

#[cfg(test)]
mod tests {
    use super::*;

    fn accents() -> [Color; 4] {
        [
            Color::rgb(255, 200, 0),
            Color::rgb(255, 0, 255),
            Color::rgb(0, 100, 255),
            Color::rgb(0, 255, 255),
        ]
    }

    #[test]
    fn test_color_cycles_then_settles() {
        let accents = accents();
        // Começa na primeira cor.
        assert_eq!(glow_color(&accents, Duration::ZERO, true), accents[0]);
        // Um quarto de volta depois está na segunda.
        assert_eq!(glow_color(&accents, CYCLE / 4, true), accents[1]);
        // No meio do caminho entre duas cores, é uma mistura das duas.
        let mid = glow_color(&accents, CYCLE / 8, true);
        assert_ne!(mid, accents[0]);
        assert_ne!(mid, accents[1]);
        // Depois da animação, para na primeira cor.
        assert_eq!(glow_color(&accents, GLOW_DURATION, true), accents[0]);
        assert_eq!(glow_color(&accents, GLOW_DURATION * 10, true), accents[0]);
    }

    #[test]
    fn test_disabled_animations_stay_still() {
        let accents = accents();
        for ms in [0, 300, 1200, 5000] {
            let age = Duration::from_millis(ms);
            assert_eq!(glow_color(&accents, age, false), accents[0]);
            assert_eq!(ring_width(age, false), 2.0);
            assert!(!is_animating(age, false));
        }
    }

    #[test]
    fn test_ring_width_pulses_while_animating() {
        for ms in (0..8000).step_by(97) {
            let width = ring_width(Duration::from_millis(ms), true);
            assert!((2.0..=4.0).contains(&width), "{width}");
        }
        assert_eq!(ring_width(GLOW_DURATION, true), 2.0);
    }

    #[test]
    fn test_styles_move_only_while_animating() {
        use crate::ui::tab_animation::AnimationSpeed;
        let accents = accents();
        let motion = |style| Motion {
            style,
            speed: AnimationSpeed::Normal,
        };
        for style in TabAnimation::ALL {
            let m = motion(style);
            // Depois do prazo, todos param na primeira cor, anel fino.
            assert_eq!(notice_color(&accents, GLOW_DURATION, m, 0.3), accents[0]);
            assert_eq!(notice_ring_width(GLOW_DURATION, m), 2.0);
        }
        // Anel: passeia como antes e pulsa.
        let ring = motion(TabAnimation::Ring);
        assert_eq!(notice_color(&accents, CYCLE / 4, ring, 0.0), accents[1]);
        assert!(notice_ring_width(CYCLE / 8, ring) > 2.0);
        // Onda: a faixa adiantada mostra outra cor no mesmo instante.
        let wave = motion(TabAnimation::Wave);
        assert_ne!(
            notice_color(&accents, Duration::ZERO, wave, 0.0),
            notice_color(&accents, Duration::ZERO, wave, 0.25)
        );
        assert_eq!(notice_ring_width(CYCLE / 8, wave), 2.0);
        // Pulso: uma cor só, mais clara e mais escura.
        let pulse = motion(TabAnimation::Pulse);
        let a = notice_color(&accents, Duration::from_millis(300), pulse, 0.0);
        let b = notice_color(&accents, Duration::from_millis(900), pulse, 0.0);
        assert_ne!(a, b);
        // Parados: ponto, nenhuma e o giratório não mudam de cor.
        for style in [TabAnimation::Dot, TabAnimation::None, TabAnimation::Spinner] {
            assert_eq!(
                notice_color(&accents, CYCLE / 4, motion(style), 0.0),
                accents[0]
            );
        }
        assert!(!is_notice_animating(
            Duration::ZERO,
            motion(TabAnimation::Dot)
        ));
        assert!(is_notice_animating(
            Duration::ZERO,
            motion(TabAnimation::Spinner)
        ));
    }

    #[test]
    fn test_degenerate_accents() {
        assert_eq!(
            glow_color(&[], Duration::ZERO, true),
            Color::rgb(255, 255, 255)
        );
        let one = [Color::rgb(1, 2, 3)];
        assert_eq!(glow_color(&one, CYCLE / 3, true), one[0]);
    }

    /// O relógio para sozinho depois do prazo: o app volta a dormir.
    #[test]
    fn test_clock_stops_after_deadline() {
        static TICKS: AtomicU64 = AtomicU64::new(0);
        fn count() {
            TICKS.fetch_add(1, Ordering::Relaxed);
        }
        let clock = WakeClock::new(count);
        clock.keep_awake(Duration::from_millis(80));
        clock.keep_awake(Duration::from_millis(120)); // só estende, não cria outro
        assert!(clock.is_running());
        let deadline = Instant::now() + Duration::from_secs(5);
        while clock.is_running() && Instant::now() < deadline {
            std::thread::sleep(Duration::from_millis(20));
        }
        assert!(!clock.is_running());
        let ticks = TICKS.load(Ordering::Relaxed);
        assert!(ticks >= 2, "{ticks} quadros");
        // Parado, não acorda mais ninguém.
        std::thread::sleep(Duration::from_millis(100));
        assert_eq!(TICKS.load(Ordering::Relaxed), ticks);
    }
}
