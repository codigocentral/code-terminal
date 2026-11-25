//! Sistema de Hooks para AI Agents
//!
//! Este módulo fornece uma API para integração de AI agents (Claude Code, OpenCode, Aider)
//! com o terminal. Permite que agents se registrem para receber eventos e executem ações.
//!
//! # Exemplo
//!
//! ```rust,ignore
//! use code_terminal::ai::hooks::{HookManager, AgentType, AgentHook};
//!
//! let mut manager = HookManager::new();
//!
//! // Registrar um agente
//! manager.register_agent(AgentType::ClaudeCode, |event| {
//!     println!("Evento recebido: {:?}", event);
//! });
//! ```

pub mod api;
pub mod context;
pub mod error;
pub mod event;
pub mod manager;
pub mod registry;
pub mod transport;
pub mod types;

// Agentes específicos
pub mod aider;
pub mod claude_code;
pub mod generic;
pub mod opencode;

pub use api::{AgentApi, AgentCapabilities, AgentCommand, AgentRequest, AgentResponse};
pub use context::{AgentContext, TerminalSnapshot, WorkspaceInfo};
pub use error::{HookError, HookResult};
pub use event::{AgentEvent, EventData, EventPriority, EventType};
pub use manager::{HookConfig, HookManager, HookStatistics};
pub use registry::{AgentRegistration, HookRegistry};
pub use types::{AgentInfo, AgentStatus, AgentType, HookPriority, HookType};

// Re-exports específicos de agentes
pub use aider::AiderHook;
pub use claude_code::ClaudeCodeHook;
pub use generic::GenericAgentHook;
pub use opencode::OpenCodeHook;
