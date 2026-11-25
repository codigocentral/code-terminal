//! Protocolo JSON-RPC 2.0 para comunicação remota
//!
//! Implementa as especificações JSON-RPC 2.0:
//! - Request/Response/Notification
//! - Batch requests
//! - Error codes padronizados
//!
//! # Exemplo
//!
//! ```
//! use code_terminal::remote::protocol::{Request, Response, Id};
//!
//! let request = Request {
//!     jsonrpc: "2.0".to_string(),
//!     id: Some(Id::Num(1)),
//!     method: "terminal.execute".to_string(),
//!     params: Some(serde_json::json!({"command": "ls"})),
//! };
//! ```

use serde::{Deserialize, Serialize};
use serde_json::Value;

/// Versão do protocolo JSON-RPC
pub const JSONRPC_VERSION: &str = "2.0";

/// Identificador de requisição
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(untagged)]
pub enum Id {
    /// ID numérico
    Num(i64),
    /// ID string
    Str(String),
    /// ID nulo (possível mas não recomendado)
    Null,
}

impl From<i64> for Id {
    fn from(n: i64) -> Self {
        Id::Num(n)
    }
}

impl From<String> for Id {
    fn from(s: String) -> Self {
        Id::Str(s)
    }
}

impl From<&str> for Id {
    fn from(s: &str) -> Self {
        Id::Str(s.to_string())
    }
}

impl std::fmt::Display for Id {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Id::Num(n) => write!(f, "{}", n),
            Id::Str(s) => write!(f, "{}", s),
            Id::Null => write!(f, "null"),
        }
    }
}

/// Notificação JSON-RPC (alias para Request sem ID)
pub type Notification = Request;

/// Requisição JSON-RPC
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct Request {
    /// Versão do protocolo (deve ser "2.0")
    pub jsonrpc: String,

    /// ID da requisição (None para notificações)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub id: Option<Id>,

    /// Método a ser chamado
    pub method: String,

    /// Parâmetros do método (opcional)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub params: Option<Value>,
}

impl Request {
    /// Criar nova requisição
    pub fn new<M: Into<String>, P: Serialize>(
        id: impl Into<Id>,
        method: M,
        params: P,
    ) -> Result<Self, serde_json::Error> {
        Ok(Self {
            jsonrpc: JSONRPC_VERSION.to_string(),
            id: Some(id.into()),
            method: method.into(),
            params: Some(serde_json::to_value(params)?),
        })
    }

    /// Criar notificação (sem ID)
    pub fn notification<M: Into<String>, P: Serialize>(
        method: M,
        params: P,
    ) -> Result<Self, serde_json::Error> {
        Ok(Self {
            jsonrpc: JSONRPC_VERSION.to_string(),
            id: None,
            method: method.into(),
            params: Some(serde_json::to_value(params)?),
        })
    }

    /// Verificar se é uma notificação (sem ID)
    pub fn is_notification(&self) -> bool {
        self.id.is_none()
    }

    /// Validar requisição
    pub fn validate(&self) -> Result<(), RpcError> {
        if self.jsonrpc != JSONRPC_VERSION {
            return Err(RpcError::invalid_request(
                "Invalid JSON-RPC version".to_string(),
            ));
        }

        if self.method.is_empty() {
            return Err(RpcError::invalid_request(
                "Method cannot be empty".to_string(),
            ));
        }

        Ok(())
    }
}

/// Resposta JSON-RPC
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct Response {
    /// Versão do protocolo
    pub jsonrpc: String,

    /// ID da requisição correspondente
    #[serde(skip_serializing_if = "Option::is_none")]
    pub id: Option<Id>,

    /// Resultado (se sucesso)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub result: Option<Value>,

    /// Erro (se falha)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<ErrorObject>,
}

impl Response {
    /// Criar resposta de sucesso
    pub fn success<R: Serialize>(id: impl Into<Id>, result: R) -> Result<Self, serde_json::Error> {
        Ok(Self {
            jsonrpc: JSONRPC_VERSION.to_string(),
            id: Some(id.into()),
            result: Some(serde_json::to_value(result)?),
            error: None,
        })
    }

    /// Criar resposta de erro
    pub fn error(id: Option<Id>, error: ErrorObject) -> Self {
        Self {
            jsonrpc: JSONRPC_VERSION.to_string(),
            id,
            result: None,
            error: Some(error),
        }
    }

    /// Criar resposta de erro a partir de RpcError
    pub fn from_error(id: Option<Id>, error: RpcError) -> Self {
        Self::error(id, error.into())
    }

    /// Verificar se é sucesso
    pub fn is_success(&self) -> bool {
        self.error.is_none()
    }

    /// Verificar se é erro
    pub fn is_error(&self) -> bool {
        self.error.is_some()
    }
}

/// Objeto de erro JSON-RPC
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ErrorObject {
    /// Código de erro
    pub code: i32,

    /// Mensagem de erro
    pub message: String,

    /// Dados adicionais (opcional)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub data: Option<Value>,
}

/// Códigos de erro JSON-RPC 2.0
pub mod error_codes {
    /// Parse error (-32700): JSON inválido
    pub const PARSE_ERROR: i32 = -32700;

    /// Invalid Request (-32600): Requisição JSON-RPC inválida
    pub const INVALID_REQUEST: i32 = -32600;

    /// Method not found (-32601): Método não existe
    pub const METHOD_NOT_FOUND: i32 = -32601;

    /// Invalid params (-32602): Parâmetros inválidos
    pub const INVALID_PARAMS: i32 = -32602;

    /// Internal error (-32603): Erro interno do servidor
    pub const INTERNAL_ERROR: i32 = -32603;

    /// Server error (-32000 to -32099): Erros definidos pelo servidor
    pub const SERVER_ERROR: i32 = -32000;

    /// Custom: Método não implementado
    pub const NOT_IMPLEMENTED: i32 = -32001;

    /// Custom: Timeout
    pub const TIMEOUT: i32 = -32002;

    /// Custom: Autenticação necessária
    pub const UNAUTHORIZED: i32 = -32003;

    /// Custom: Permissão negada
    pub const FORBIDDEN: i32 = -32004;
}

/// Erro JSON-RPC
#[derive(Debug, Clone, PartialEq)]
pub enum RpcError {
    /// Erro de parse (-32700)
    ParseError(String),

    /// Requisição inválida (-32600)
    InvalidRequest(String),

    /// Método não encontrado (-32601)
    MethodNotFound(String),

    /// Parâmetros inválidos (-32602)
    InvalidParams(String),

    /// Erro interno (-32603)
    InternalError(String),

    /// Erro do servidor (-32000)
    ServerError(i32, String, Option<Value>),
}

impl RpcError {
    /// Criar erro de parse
    pub fn parse_error(msg: impl Into<String>) -> Self {
        RpcError::ParseError(msg.into())
    }

    /// Criar erro de requisição inválida
    pub fn invalid_request(msg: impl Into<String>) -> Self {
        RpcError::InvalidRequest(msg.into())
    }

    /// Criar erro de método não encontrado
    pub fn method_not_found(method: impl Into<String>) -> Self {
        RpcError::MethodNotFound(method.into())
    }

    /// Criar erro de parâmetros inválidos
    pub fn invalid_params(msg: impl Into<String>) -> Self {
        RpcError::InvalidParams(msg.into())
    }

    /// Criar erro interno
    pub fn internal_error(msg: impl Into<String>) -> Self {
        RpcError::InternalError(msg.into())
    }

    /// Criar erro de servidor customizado
    pub fn server_error(code: i32, msg: impl Into<String>, data: Option<Value>) -> Self {
        RpcError::ServerError(code, msg.into(), data)
    }

    /// Retornar código de erro
    pub fn code(&self) -> i32 {
        use error_codes::*;
        match self {
            RpcError::ParseError(_) => PARSE_ERROR,
            RpcError::InvalidRequest(_) => INVALID_REQUEST,
            RpcError::MethodNotFound(_) => METHOD_NOT_FOUND,
            RpcError::InvalidParams(_) => INVALID_PARAMS,
            RpcError::InternalError(_) => INTERNAL_ERROR,
            RpcError::ServerError(code, _, _) => *code,
        }
    }

    /// Retornar mensagem de erro
    pub fn message(&self) -> &str {
        match self {
            RpcError::ParseError(msg) => msg,
            RpcError::InvalidRequest(msg) => msg,
            RpcError::MethodNotFound(msg) => msg,
            RpcError::InvalidParams(msg) => msg,
            RpcError::InternalError(msg) => msg,
            RpcError::ServerError(_, msg, _) => msg,
        }
    }

    /// Retornar dados adicionais (se houver)
    pub fn data(&self) -> Option<&Value> {
        match self {
            RpcError::ServerError(_, _, data) => data.as_ref(),
            _ => None,
        }
    }
}

impl std::fmt::Display for RpcError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "JSON-RPC Error ({}): {}", self.code(), self.message())
    }
}

impl std::error::Error for RpcError {}

impl From<RpcError> for ErrorObject {
    fn from(err: RpcError) -> Self {
        ErrorObject {
            code: err.code(),
            message: err.message().to_string(),
            data: err.data().cloned(),
        }
    }
}

/// Mensagem JSON-RPC (requisição, resposta, ou batch)
#[derive(Debug, Clone, PartialEq)]
pub enum Message {
    /// Requisição única
    Request(Request),
    /// Resposta única
    Response(Response),
    /// Batch de requisições
    BatchRequest(Vec<Request>),
    /// Batch de respostas
    BatchResponse(Vec<Response>),
}

impl Message {
    /// Parse de string JSON
    pub fn from_str(json: &str) -> Result<Self, RpcError> {
        // Tentar parse como batch primeiro (array)
        if json.trim().starts_with('[') {
            let requests: Vec<Request> =
                serde_json::from_str(json).map_err(|e| RpcError::parse_error(e.to_string()))?;
            return Ok(Message::BatchRequest(requests));
        }

        // Tentar parse como requisição ou resposta
        let value: Value =
            serde_json::from_str(json).map_err(|e| RpcError::parse_error(e.to_string()))?;

        // Verificar se é requisição (tem "method") ou resposta (tem "result" ou "error")
        if value.get("method").is_some() {
            let request: Request = serde_json::from_value(value)
                .map_err(|e| RpcError::invalid_request(e.to_string()))?;
            Ok(Message::Request(request))
        } else {
            let response: Response = serde_json::from_value(value)
                .map_err(|e| RpcError::invalid_request(e.to_string()))?;
            Ok(Message::Response(response))
        }
    }

    /// Converter para string JSON
    pub fn to_json_string(&self) -> Result<String, serde_json::Error> {
        match self {
            Message::Request(r) => serde_json::to_string(r),
            Message::Response(r) => serde_json::to_string(r),
            Message::BatchRequest(r) => serde_json::to_string(r),
            Message::BatchResponse(r) => serde_json::to_string(r),
        }
    }
}

/// Parser de mensagens newline-delimited (NDJSON)
pub struct MessageParser;

impl MessageParser {
    /// Parse de uma linha (single message)
    pub fn parse_line(line: &str) -> Result<Message, RpcError> {
        Message::from_str(line.trim())
    }

    /// Parse de múltiplas linhas
    pub fn parse_batch(lines: &str) -> Vec<Result<Message, RpcError>> {
        lines
            .lines()
            .filter(|l| !l.trim().is_empty())
            .map(Self::parse_line)
            .collect()
    }

    /// Verificar se linha é uma mensagem JSON-RPC válida
    pub fn is_valid_message(line: &str) -> bool {
        Self::parse_line(line).is_ok()
    }
}

/// Builder para requisições
#[derive(Debug, Default)]
pub struct RequestBuilder {
    id: Option<Id>,
    method: Option<String>,
    params: Option<Value>,
}

impl RequestBuilder {
    /// Criar novo builder
    pub fn new() -> Self {
        Self::default()
    }

    /// Definir ID
    pub fn id(mut self, id: impl Into<Id>) -> Self {
        self.id = Some(id.into());
        self
    }

    /// Definir método
    pub fn method(mut self, method: impl Into<String>) -> Self {
        self.method = Some(method.into());
        self
    }

    /// Definir parâmetros
    pub fn params<P: Serialize>(mut self, params: P) -> Result<Self, serde_json::Error> {
        self.params = Some(serde_json::to_value(params)?);
        Ok(self)
    }

    /// Construir requisição
    pub fn build(self) -> Result<Request, RpcError> {
        let method = self
            .method
            .ok_or_else(|| RpcError::invalid_request("Method is required"))?;

        Ok(Request {
            jsonrpc: JSONRPC_VERSION.to_string(),
            id: self.id,
            method,
            params: self.params,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_id_creation() {
        let id: Id = 42.into();
        assert_eq!(id, Id::Num(42));

        let id: Id = "test".into();
        assert_eq!(id, Id::Str("test".to_string()));

        assert_eq!(id.to_string(), "test");
        assert_eq!(Id::Num(42).to_string(), "42");
    }

    #[test]
    fn test_request_creation() {
        let req = Request::new(1, "test.method", serde_json::json!({"key": "value"})).unwrap();
        assert_eq!(req.jsonrpc, "2.0");
        assert_eq!(req.id, Some(Id::Num(1)));
        assert_eq!(req.method, "test.method");
        assert!(!req.is_notification());
    }

    #[test]
    fn test_notification_creation() {
        let notif = Request::notification("test.event", serde_json::json!({})).unwrap();
        assert!(notif.is_notification());
        assert_eq!(notif.id, None);
    }

    #[test]
    fn test_request_validation() {
        let valid = Request {
            jsonrpc: "2.0".to_string(),
            id: Some(Id::Num(1)),
            method: "test".to_string(),
            params: None,
        };
        assert!(valid.validate().is_ok());

        let invalid_version = Request {
            jsonrpc: "1.0".to_string(),
            id: Some(Id::Num(1)),
            method: "test".to_string(),
            params: None,
        };
        assert!(invalid_version.validate().is_err());

        let empty_method = Request {
            jsonrpc: "2.0".to_string(),
            id: Some(Id::Num(1)),
            method: "".to_string(),
            params: None,
        };
        assert!(empty_method.validate().is_err());
    }

    #[test]
    fn test_response_success() {
        let resp = Response::success(1, serde_json::json!({"result": "ok"})).unwrap();
        assert!(resp.is_success());
        assert!(!resp.is_error());
        assert_eq!(resp.id, Some(Id::Num(1)));
    }

    #[test]
    fn test_response_error() {
        let error = ErrorObject {
            code: error_codes::METHOD_NOT_FOUND,
            message: "Method not found".to_string(),
            data: None,
        };
        let resp = Response::error(Some(Id::Num(1)), error);
        assert!(!resp.is_success());
        assert!(resp.is_error());
    }

    #[test]
    fn test_rpc_error_codes() {
        let err = RpcError::parse_error("test");
        assert_eq!(err.code(), error_codes::PARSE_ERROR);

        let err = RpcError::method_not_found("test");
        assert_eq!(err.code(), error_codes::METHOD_NOT_FOUND);

        let err = RpcError::internal_error("test");
        assert_eq!(err.code(), error_codes::INTERNAL_ERROR);

        let err = RpcError::server_error(-32001, "test", None);
        assert_eq!(err.code(), -32001);
    }

    #[test]
    fn test_message_from_str_request() {
        let json = r#"{"jsonrpc":"2.0","id":1,"method":"test","params":{}}"#;
        let msg = Message::from_str(json).unwrap();
        assert!(matches!(msg, Message::Request(_)));
    }

    #[test]
    fn test_message_from_str_response() {
        let json = r#"{"jsonrpc":"2.0","id":1,"result":"ok"}"#;
        let msg = Message::from_str(json).unwrap();
        assert!(matches!(msg, Message::Response(_)));
    }

    #[test]
    fn test_message_from_str_batch() {
        let json = r#"[{"jsonrpc":"2.0","id":1,"method":"test"}]"#;
        let msg = Message::from_str(json).unwrap();
        assert!(matches!(msg, Message::BatchRequest(_)));
    }

    #[test]
    fn test_message_parser() {
        let line = r#"{"jsonrpc":"2.0","id":1,"method":"test"}"#;
        assert!(MessageParser::is_valid_message(line));

        let invalid = "not json";
        assert!(!MessageParser::is_valid_message(invalid));
    }

    #[test]
    fn test_request_builder() {
        let req = RequestBuilder::new()
            .id(42)
            .method("test.method")
            .params(serde_json::json!({"key": "value"}))
            .unwrap()
            .build()
            .unwrap();

        assert_eq!(req.id, Some(Id::Num(42)));
        assert_eq!(req.method, "test.method");
    }

    #[test]
    fn test_request_builder_missing_method() {
        let result = RequestBuilder::new().id(1).build();
        assert!(result.is_err());
    }

    #[test]
    fn test_error_object_serialization() {
        let error = ErrorObject {
            code: -32600,
            message: "Invalid Request".to_string(),
            data: Some(serde_json::json!({"details": "test"})),
        };
        let json = serde_json::to_string(&error).unwrap();
        assert!(json.contains("-32600"));
        assert!(json.contains("Invalid Request"));
    }

    #[test]
    fn test_serde_roundtrip() {
        let original = Request {
            jsonrpc: "2.0".to_string(),
            id: Some(Id::Num(42)),
            method: "test.method".to_string(),
            params: Some(serde_json::json!({"key": "value"})),
        };

        let json = serde_json::to_string(&original).unwrap();
        let parsed: Request = serde_json::from_str(&json).unwrap();
        assert_eq!(original, parsed);
    }
}
