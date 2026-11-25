//! Testes de Integração - PaneId Unificado
//!
//! Este módulo testa o tipo PaneId unificado usado em todo o sistema,
//! verificando que é consistentemente u64 em todos os módulos.
//!
//! # Execução
//!
//! ```bash
//! cargo test --test pane_id_test
//! ```

use std::collections::HashSet;
use std::sync::atomic::{AtomicU64, Ordering};

/// Teste 1: PaneId é u64
///
/// Verifica que PaneId é um alias para u64.
#[test]
fn test_pane_id_is_u64() {
    use code_terminal::types::PaneId;

    // Verifica tamanho
    assert_eq!(std::mem::size_of::<PaneId>(), std::mem::size_of::<u64>());

    // Verifica que pode ser usado como u64
    let id: PaneId = 42;
    let val: u64 = id;
    assert_eq!(val, 42);

    // Operações aritméticas
    let id2 = id + 1;
    assert_eq!(id2, 43);
}

/// Teste 2: PaneId é consistente entre módulos
///
/// Verifica que todos os módulos usam o mesmo PaneId.
#[test]
fn test_pane_id_consistency() {
    use code_terminal::pane::PaneId as PaneModuleId;
    use code_terminal::types::PaneId as TypesPaneId;
    use code_terminal::ui::layout::PaneId as LayoutPaneId;

    // Todos devem ser o mesmo tipo (u64)
    let id1: TypesPaneId = 1;
    let id2: LayoutPaneId = 1;
    let id3: PaneModuleId = 1;

    assert_eq!(id1, id2);
    assert_eq!(id2, id3);

    // Todos devem ter o mesmo tamanho
    assert_eq!(
        std::mem::size_of::<TypesPaneId>(),
        std::mem::size_of::<u64>()
    );
    assert_eq!(
        std::mem::size_of::<LayoutPaneId>(),
        std::mem::size_of::<u64>()
    );
    assert_eq!(
        std::mem::size_of::<PaneModuleId>(),
        std::mem::size_of::<u64>()
    );
}

/// Teste 3: Geração de IDs únicos
///
/// Verifica que IDs podem ser gerados sequencialmente.
#[test]
fn test_unique_id_generation() {
    use code_terminal::types::{next_pane_id, PaneId};

    let mut counter: PaneId = 0;

    let id1 = next_pane_id(&mut counter);
    let id2 = next_pane_id(&mut counter);
    let id3 = next_pane_id(&mut counter);

    assert_eq!(id1, 0);
    assert_eq!(id2, 1);
    assert_eq!(id3, 2);
    assert_eq!(counter, 3);
}

/// Teste 4: IDs únicos em múltiplas threads
///
/// Verifica que AtomicU64 pode ser usado para IDs thread-safe.
#[test]
fn test_thread_safe_id_generation() {
    use std::sync::Arc;
    use std::thread;

    let counter = Arc::new(AtomicU64::new(1));
    let mut handles = vec![];
    let mut ids = vec![];

    // Cria múltiplas threads gerando IDs
    for _ in 0..10 {
        let counter = Arc::clone(&counter);
        let handle = thread::spawn(move || counter.fetch_add(1, Ordering::SeqCst));
        handles.push(handle);
    }

    // Coleta resultados
    for handle in handles {
        ids.push(handle.join().unwrap());
    }

    // Todos devem ser únicos
    let unique: HashSet<u64> = ids.iter().cloned().collect();
    assert_eq!(unique.len(), 10, "Todos os IDs devem ser únicos");
}

/// Teste 5: Display e Debug para PaneId
///
/// Verifica formatação do ID.
#[test]
fn test_pane_id_formatting() {
    use code_terminal::types::PaneId;

    let id: PaneId = 12345;

    // Display
    let display = format!("{}", id);
    assert_eq!(display, "12345");

    // Debug
    let debug = format!("{:?}", id);
    assert_eq!(debug, "12345");

    // Binary
    let binary = format!("{:b}", id);
    assert_eq!(binary, "11000000111001");

    // Hex
    let hex = format!("{:x}", id);
    assert_eq!(hex, "3039");
}

/// Teste 6: PaneId em coleções
///
/// Verifica que PaneId funciona em HashMap e HashSet.
#[test]
fn test_pane_id_in_collections() {
    use code_terminal::types::PaneId;
    use std::collections::HashMap;

    let mut map: HashMap<PaneId, String> = HashMap::new();

    map.insert(1, "Pane 1".to_string());
    map.insert(2, "Pane 2".to_string());
    map.insert(3, "Pane 3".to_string());

    assert_eq!(map.get(&1), Some(&"Pane 1".to_string()));
    assert_eq!(map.get(&2), Some(&"Pane 2".to_string()));

    // HashSet
    let mut set: HashSet<PaneId> = HashSet::new();
    set.insert(1);
    set.insert(2);
    set.insert(1); // Duplicado

    assert_eq!(set.len(), 2);
}

/// Teste 7: Conversões de tipo
///
/// Verifica conversões entre PaneId e outros tipos numéricos.
#[test]
fn test_pane_id_conversions() {
    use code_terminal::types::PaneId;

    let id: PaneId = 100;

    // Para tipos menores (com cuidado)
    let as_u32: u32 = id as u32;
    assert_eq!(as_u32, 100);

    let as_usize: usize = id as usize;
    assert_eq!(as_usize, 100);

    // De tipos menores
    let from_u32: PaneId = 50 as PaneId;
    assert_eq!(from_u32, 50);
}

/// Teste 8: Limites de PaneId
///
/// Verifica valores máximos e mínimos.
#[test]
fn test_pane_id_limits() {
    use code_terminal::types::PaneId;

    // Mínimo
    let min: PaneId = 0;
    assert_eq!(min, 0);

    // Máximo u64
    let max: PaneId = u64::MAX;
    assert_eq!(max, u64::MAX);

    // Overflow não deve panico (wrap around em release)
    let large = u64::MAX;
    let _wrapped = large.wrapping_add(1);
}

/// Teste 9: TabId também é u64
///
/// Verifica que TabId segue o mesmo padrão.
#[test]
fn test_tab_id_is_u64() {
    use code_terminal::types::TabId;

    assert_eq!(std::mem::size_of::<TabId>(), std::mem::size_of::<u64>());

    let id: TabId = 42;
    let val: u64 = id;
    assert_eq!(val, 42);
}

/// Teste 10: WindowId também é u64
///
/// Verifica que WindowId segue o mesmo padrão.
#[test]
fn test_window_id_is_u64() {
    use code_terminal::types::WindowId;

    assert_eq!(std::mem::size_of::<WindowId>(), std::mem::size_of::<u64>());

    let id: WindowId = 42;
    let val: u64 = id;
    assert_eq!(val, 42);
}

/// Teste 11: SessionId também é u64
///
/// Verifica que SessionId segue o mesmo padrão.
#[test]
fn test_session_id_is_u64() {
    use code_terminal::types::SessionId;

    assert_eq!(std::mem::size_of::<SessionId>(), std::mem::size_of::<u64>());

    let id: SessionId = 42;
    let val: u64 = id;
    assert_eq!(val, 42);
}

/// Teste 12: Comparação de IDs
///
/// Verifica ordenação de PaneIds.
#[test]
fn test_pane_id_ordering() {
    use code_terminal::types::PaneId;

    let id1: PaneId = 10;
    let id2: PaneId = 20;
    let id3: PaneId = 10;

    assert!(id1 < id2);
    assert!(id2 > id1);
    assert_eq!(id1, id3);
    assert!(id1 <= id3);
    assert!(id1 >= id3);
}

/// Teste 13: Operações aritméticas
///
/// Verifica operações matemáticas com PaneId.
#[test]
fn test_pane_id_arithmetic() {
    use code_terminal::types::PaneId;

    let base: PaneId = 100;

    // Adição
    assert_eq!(base + 10, 110);

    // Subtração
    assert_eq!(base - 10, 90);

    // Multiplicação
    assert_eq!(base * 2, 200);

    // Divisão
    assert_eq!(base / 4, 25);

    // Módulo
    assert_eq!(base % 30, 10);

    // Incremento
    let mut id = base;
    id += 1;
    assert_eq!(id, 101);
}

/// Teste 14: Geração em massa de IDs
///
/// Verifica que muitos IDs podem ser gerados sem colisões.
#[test]
fn test_mass_id_generation() {
    use code_terminal::types::PaneId;

    const COUNT: usize = 100_000;
    let mut ids = Vec::with_capacity(COUNT);

    for i in 0..COUNT {
        ids.push(i as PaneId);
    }

    // Verifica unicidade
    let set: HashSet<PaneId> = ids.iter().cloned().collect();
    assert_eq!(set.len(), COUNT);
}

/// Teste 15: PaneId como índice
///
/// Verifica que PaneId pode ser usado como índice de array/vec.
#[test]
fn test_pane_id_as_index() {
    use code_terminal::types::PaneId;

    let data = ["pane0", "pane1", "pane2", "pane3"];

    let index: PaneId = 2;
    assert_eq!(data[index as usize], "pane2");
}

/// Teste 16: Zero como ID válido
///
/// Verifica que 0 é um ID válido.
#[test]
fn test_zero_is_valid_id() {
    use code_terminal::types::PaneId;

    let zero: PaneId = 0;
    assert_eq!(zero, 0);
    assert!(zero == 0);
}

/// Teste 17: ID máximo
///
/// Verifica comportamento com ID máximo.
#[test]
fn test_max_id() {
    use code_terminal::types::PaneId;

    let max: PaneId = u64::MAX;
    assert_eq!(max, u64::MAX);

    // Verifica que pode ser usado em HashMap
    use std::collections::HashMap;
    let mut map: HashMap<PaneId, String> = HashMap::new();
    map.insert(max, "max pane".to_string());
    assert_eq!(map.get(&max), Some(&"max pane".to_string()));
}

/// Teste 18: Traits derivados
///
/// Verifica que PaneId implementa traits essenciais.
#[test]
fn test_pane_id_traits() {
    use code_terminal::types::PaneId;

    // Copy
    let id: PaneId = 42;
    let copy = id;
    assert_eq!(id, copy);

    // Clone
    let clone = id;
    assert_eq!(id, clone);

    // Debug
    let _ = format!("{:?}", id);

    // Display
    let _ = format!("{}", id);

    // PartialEq
    assert_eq!(id, 42);

    // Eq
    fn assert_eq<T: Eq>(_: T) {}
    assert_eq(id);

    // PartialOrd
    assert!(id < 100);

    // Ord
    fn assert_ord<T: Ord>(a: T, b: T) -> std::cmp::Ordering {
        a.cmp(&b)
    }
    assert_eq!(assert_ord(id, 50), std::cmp::Ordering::Less);

    // Hash
    use std::collections::hash_map::DefaultHasher;
    use std::hash::{Hash, Hasher};
    let mut hasher = DefaultHasher::new();
    id.hash(&mut hasher);
    let _hash = hasher.finish();
}

/// Teste 19: Compatibilidade com AtomicU64
///
/// Verifica que PaneId pode ser usado com operações atômicas.
#[test]
fn test_pane_id_atomic() {
    use code_terminal::types::PaneId;

    static COUNTER: AtomicU64 = AtomicU64::new(0);

    fn next_id() -> PaneId {
        COUNTER.fetch_add(1, Ordering::SeqCst)
    }

    let id1 = next_id();
    let id2 = next_id();

    assert_eq!(id1, 0);
    assert_eq!(id2, 1);
}

/// Teste 20: PaneId em structs
///
/// Verifica que PaneId pode ser usado em estruturas.
#[test]
fn test_pane_id_in_structs() {
    use code_terminal::types::PaneId;

    #[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
    struct PaneInfo {
        id: PaneId,
        name: &'static str,
    }

    let info = PaneInfo {
        id: 42,
        name: "test",
    };

    assert_eq!(info.id, 42);

    // Usa em HashSet
    let mut set: HashSet<PaneInfo> = HashSet::new();
    set.insert(info);
    assert!(set.contains(&info));
}

/// Teste 21: From/Into implícitos
///
/// Verifica conversões automáticas.
#[test]
fn test_pane_id_conversions_implicit() {
    use code_terminal::types::PaneId;

    // i32 pode converter para PaneId
    let from_i32: PaneId = 42i32 as PaneId;
    assert_eq!(from_i32, 42);

    // isize
    let from_isize: PaneId = 42isize as PaneId;
    assert_eq!(from_isize, 42);

    // usize
    let from_usize: PaneId = 42usize as PaneId;
    assert_eq!(from_usize, 42);
}

/// Teste 22: Estresse de comparações
///
/// Verifica comparações em massa.
#[test]
fn test_pane_id_comparison_stress() {
    use code_terminal::types::PaneId;

    const COUNT: usize = 10_000;

    for i in 0..COUNT {
        let id: PaneId = i as PaneId;
        assert_eq!(id, i as PaneId);
        assert!(id <= i as PaneId);
        assert!(id >= i as PaneId);

        if i > 0 {
            assert!(id > (i - 1) as PaneId);
        }
        if i < COUNT - 1 {
            assert!(id < (i + 1) as PaneId);
        }
    }
}
