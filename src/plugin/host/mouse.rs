//! Host Functions - Mouse
//!
//! Funções para controle e leitura do mouse.

use super::{
    HostFunction, HostFunctionCategory, HostFunctionError, HostFunctionId, HostFunctionPermission,
    HostFunctionSignature, HostParamType, HostReturnType,
};
use std::sync::atomic::{AtomicI32, Ordering};

static MOUSE_X: AtomicI32 = AtomicI32::new(0);
static MOUSE_Y: AtomicI32 = AtomicI32::new(0);

pub fn all_functions() -> Vec<Box<dyn HostFunction>> {
    vec![
        Box::new(GetMousePos),
        Box::new(SetMousePos),
        Box::new(SimulateMouseClick),
        Box::new(SimulateMouseScroll),
    ]
}

pub struct GetMousePos;

impl HostFunction for GetMousePos {
    fn id(&self) -> HostFunctionId {
        HostFunctionId {
            category: HostFunctionCategory::Ui,
            name: "mouse_get_position",
        }
    }

    fn signature(&self) -> HostFunctionSignature {
        HostFunctionSignature {
            params: vec![HostParamType::Pointer], // out: (x, y)
            return_type: Some(HostReturnType::I32),
        }
    }

    fn permission(&self) -> HostFunctionPermission {
        HostFunctionPermission::Safe
    }

    fn description(&self) -> &'static str {
        "Obtém posição atual do mouse (out_ptr) -> [x: i32, y: i32]"
    }

    fn execute(&self, _params: &[u8]) -> Result<Vec<u8>, HostFunctionError> {
        let x = MOUSE_X.load(Ordering::Relaxed);
        let y = MOUSE_Y.load(Ordering::Relaxed);

        let mut result = Vec::with_capacity(8);
        result.extend_from_slice(&x.to_le_bytes());
        result.extend_from_slice(&y.to_le_bytes());

        Ok(result)
    }
}

pub struct SetMousePos;

impl HostFunction for SetMousePos {
    fn id(&self) -> HostFunctionId {
        HostFunctionId {
            category: HostFunctionCategory::Ui,
            name: "mouse_set_position",
        }
    }

    fn signature(&self) -> HostFunctionSignature {
        HostFunctionSignature {
            params: vec![HostParamType::I32, HostParamType::I32], // x, y
            return_type: Some(HostReturnType::I32),
        }
    }

    fn permission(&self) -> HostFunctionPermission {
        HostFunctionPermission::Dangerous
    }

    fn description(&self) -> &'static str {
        "Define posição do mouse (x, y) -> 0=sucesso"
    }

    fn execute(&self, params: &[u8]) -> Result<Vec<u8>, HostFunctionError> {
        if params.len() < 8 {
            return Err(HostFunctionError::InvalidParams);
        }

        let x = i32::from_le_bytes([params[0], params[1], params[2], params[3]]);
        let y = i32::from_le_bytes([params[4], params[5], params[6], params[7]]);

        MOUSE_X.store(x, Ordering::Relaxed);
        MOUSE_Y.store(y, Ordering::Relaxed);

        Ok(0i32.to_le_bytes().to_vec())
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u32)]
pub enum MouseButton {
    Left = 0,
    Right = 1,
    Middle = 2,
}

impl MouseButton {
    pub fn from_u32(value: u32) -> Option<Self> {
        match value {
            0 => Some(Self::Left),
            1 => Some(Self::Right),
            2 => Some(Self::Middle),
            _ => None,
        }
    }
}

pub struct SimulateMouseClick;

impl HostFunction for SimulateMouseClick {
    fn id(&self) -> HostFunctionId {
        HostFunctionId {
            category: HostFunctionCategory::Ui,
            name: "mouse_click",
        }
    }

    fn signature(&self) -> HostFunctionSignature {
        HostFunctionSignature {
            params: vec![
                HostParamType::I32, // button (0=left, 1=right, 2=middle)
                HostParamType::I32, // x
                HostParamType::I32, // y
            ],
            return_type: Some(HostReturnType::I32),
        }
    }

    fn permission(&self) -> HostFunctionPermission {
        HostFunctionPermission::Dangerous
    }

    fn description(&self) -> &'static str {
        "Simula clique do mouse (button, x, y) -> 0=sucesso"
    }

    fn execute(&self, params: &[u8]) -> Result<Vec<u8>, HostFunctionError> {
        if params.len() < 12 {
            return Err(HostFunctionError::InvalidParams);
        }

        let button = i32::from_le_bytes([params[0], params[1], params[2], params[3]]);
        let x = i32::from_le_bytes([params[4], params[5], params[6], params[7]]);
        let y = i32::from_le_bytes([params[8], params[9], params[10], params[11]]);

        // Valida botão
        if MouseButton::from_u32(button as u32).is_none() {
            return Err(HostFunctionError::InvalidParams);
        }

        // Atualiza posição
        MOUSE_X.store(x, Ordering::Relaxed);
        MOUSE_Y.store(y, Ordering::Relaxed);

        // Em implementação real, simularia o clique via API do sistema

        Ok(0i32.to_le_bytes().to_vec())
    }
}

pub struct SimulateMouseScroll;

impl HostFunction for SimulateMouseScroll {
    fn id(&self) -> HostFunctionId {
        HostFunctionId {
            category: HostFunctionCategory::Ui,
            name: "mouse_scroll",
        }
    }

    fn signature(&self) -> HostFunctionSignature {
        HostFunctionSignature {
            params: vec![
                HostParamType::I32, // delta_x
                HostParamType::I32, // delta_y
            ],
            return_type: Some(HostReturnType::I32),
        }
    }

    fn permission(&self) -> HostFunctionPermission {
        HostFunctionPermission::Restricted
    }

    fn description(&self) -> &'static str {
        "Simula scroll do mouse (delta_x, delta_y) -> 0=sucesso"
    }

    fn execute(&self, params: &[u8]) -> Result<Vec<u8>, HostFunctionError> {
        if params.len() < 8 {
            return Err(HostFunctionError::InvalidParams);
        }

        let _delta_x = i32::from_le_bytes([params[0], params[1], params[2], params[3]]);
        let _delta_y = i32::from_le_bytes([params[4], params[5], params[6], params[7]]);

        // Em implementação real, simularia o scroll

        Ok(0i32.to_le_bytes().to_vec())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_all_functions_exist() {
        let functions = all_functions();
        assert_eq!(functions.len(), 4);
    }

    #[test]
    fn test_mouse_button_from_u32() {
        assert_eq!(MouseButton::from_u32(0), Some(MouseButton::Left));
        assert_eq!(MouseButton::from_u32(1), Some(MouseButton::Right));
        assert_eq!(MouseButton::from_u32(2), Some(MouseButton::Middle));
        assert_eq!(MouseButton::from_u32(99), None);
    }

    #[test]
    fn test_mouse_position() {
        // Testa que podemos setar e obter posição
        let set_fn = SetMousePos;
        let get_fn = GetMousePos;

        let result = set_fn
            .execute(&[100i32.to_le_bytes(), 200i32.to_le_bytes()].concat())
            .unwrap();
        assert_eq!(result, 0i32.to_le_bytes().to_vec());

        let pos = get_fn.execute(&[]).unwrap();
        let x = i32::from_le_bytes([pos[0], pos[1], pos[2], pos[3]]);
        let y = i32::from_le_bytes([pos[4], pos[5], pos[6], pos[7]]);

        assert_eq!(x, 100);
        assert_eq!(y, 200);
    }

    #[test]
    fn test_mouse_click_invalid_button() {
        let click = SimulateMouseClick;

        // Botão inválido (99)
        let params = [99i32.to_le_bytes(), 0i32.to_le_bytes(), 0i32.to_le_bytes()].concat();
        assert!(click.execute(&params).is_err());
    }
}
