//! Session Client - Cliente para conectar ao session server

use crate::multiplexer::transport::MuxConnection;
use std::io::{Read, Write};
use std::sync::atomic::{AtomicU64, Ordering};

use super::protocol::{ErrorCode, Message, MessageType, SessionInfo};
use super::session::SessionId;
use super::{MultiplexerError, MultiplexerResult};

/// Cliente de sessão
pub struct SessionClient {
    /// Conexão com o daemon (socket Unix ou TCP)
    stream: MuxConnection,
    /// Contador de mensagens
    message_counter: AtomicU64,
    /// ID da sessão atual (se anexado)
    current_session: Option<SessionId>,
    /// Bytes já lidos que ainda não formam uma linha completa
    pending: Vec<u8>,
}

impl SessionClient {
    /// Conecta ao daemon local (socket Unix; TCP 127.0.0.1:9876 no Windows).
    pub fn connect() -> MultiplexerResult<Self> {
        let stream = MuxConnection::connect_default()
            .map_err(|e| MultiplexerError::ServerError(format!("Failed to connect: {}", e)))?;
        Ok(Self::with_connection(stream))
    }

    /// Conecta a um endereço TCP específico
    pub fn connect_to(addr: &str) -> MultiplexerResult<Self> {
        let stream = MuxConnection::connect_tcp(addr)
            .map_err(|e| MultiplexerError::ServerError(format!("Failed to connect: {}", e)))?;
        Ok(Self::with_connection(stream))
    }

    /// Usa uma conexão já aberta.
    pub fn with_connection(stream: MuxConnection) -> Self {
        Self {
            stream,
            message_counter: AtomicU64::new(1),
            current_session: None,
            pending: Vec::new(),
        }
    }

    /// Verifica se o servidor está disponível
    pub fn ping(&mut self) -> MultiplexerResult<bool> {
        let response = self.send_request(MessageType::Ping)?;
        match response.msg_type {
            MessageType::Pong => Ok(true),
            _ => Ok(false),
        }
    }

    /// Cria uma nova sessão
    pub fn create_session(
        &mut self,
        name: Option<String>,
        size: (u16, u16),
    ) -> MultiplexerResult<(SessionId, String)> {
        let response = self.send_request(MessageType::CreateSession { name, size })?;

        match response.msg_type {
            MessageType::SessionCreated { session_id, name } => {
                let id = parse_session_id(&session_id)?;
                Ok((id, name))
            }
            MessageType::Error { code, message } => Err(MultiplexerError::ServerError(format!(
                "{:?}: {}",
                code, message
            ))),
            _ => Err(MultiplexerError::ServerError(
                "Unexpected response".to_string(),
            )),
        }
    }

    /// Anexa a uma sessão existente
    pub fn attach(
        &mut self,
        session_id: &str,
        read_only: bool,
    ) -> MultiplexerResult<(SessionId, String)> {
        let response = self.send_request(MessageType::Attach {
            session_id: session_id.to_string(),
            read_only,
        })?;

        match response.msg_type {
            MessageType::Attached { session_id, name } => {
                let id = parse_session_id(&session_id)?;
                self.current_session = Some(id);
                Ok((id, name))
            }
            MessageType::Error { code, message } => Err(map_error(code, message)),
            _ => Err(MultiplexerError::ServerError(
                "Unexpected response".to_string(),
            )),
        }
    }

    /// Desanexa da sessão atual
    pub fn detach(&mut self) -> MultiplexerResult<()> {
        let response = self.send_request(MessageType::Detach)?;

        match response.msg_type {
            MessageType::Detached { .. } => {
                self.current_session = None;
                Ok(())
            }
            MessageType::Error {
                code: ErrorCode::NotAttached,
                ..
            } => {
                // Já estava desanexado, não é erro
                Ok(())
            }
            MessageType::Error { code, message } => Err(map_error(code, message)),
            _ => Err(MultiplexerError::ServerError(
                "Unexpected response".to_string(),
            )),
        }
    }

    /// Lista todas as sessões
    pub fn list_sessions(&mut self) -> MultiplexerResult<Vec<SessionInfo>> {
        let response = self.send_request(MessageType::ListSessions)?;

        match response.msg_type {
            MessageType::SessionList { sessions } => Ok(sessions),
            MessageType::Error { code, message } => Err(MultiplexerError::ServerError(format!(
                "{:?}: {}",
                code, message
            ))),
            _ => Err(MultiplexerError::ServerError(
                "Unexpected response".to_string(),
            )),
        }
    }

    /// Renomeia uma sessão
    pub fn rename_session(&mut self, session_id: &str, new_name: String) -> MultiplexerResult<()> {
        let response = self.send_request(MessageType::RenameSession {
            session_id: session_id.to_string(),
            new_name,
        })?;

        match response.msg_type {
            MessageType::SessionRenamed { .. } => Ok(()),
            MessageType::Error { code, message } => Err(map_error(code, message)),
            _ => Err(MultiplexerError::ServerError(
                "Unexpected response".to_string(),
            )),
        }
    }

    /// Destrói uma sessão
    pub fn destroy_session(&mut self, session_id: &str) -> MultiplexerResult<()> {
        let response = self.send_request(MessageType::DestroySession {
            session_id: session_id.to_string(),
        })?;

        match response.msg_type {
            MessageType::SessionDestroyed { .. } => Ok(()),
            MessageType::Error { code, message } => Err(map_error(code, message)),
            _ => Err(MultiplexerError::ServerError(
                "Unexpected response".to_string(),
            )),
        }
    }

    /// Envia dados de entrada para a sessão
    pub fn send_input(&mut self, data: &[u8]) -> MultiplexerResult<()> {
        let response = self.send_request(MessageType::Input {
            data: data.to_vec(),
        })?;

        match response.msg_type {
            MessageType::Error { code, message } => Err(map_error(code, message)),
            _ => Ok(()),
        }
    }

    /// Redimensiona o terminal
    pub fn resize(&mut self, cols: u16, rows: u16) -> MultiplexerResult<()> {
        let response = self.send_request(MessageType::Resize { cols, rows })?;

        match response.msg_type {
            MessageType::Error { code, message } => Err(map_error(code, message)),
            _ => Ok(()),
        }
    }

    /// Retorna a sessão atual
    pub fn current_session(&self) -> Option<SessionId> {
        self.current_session
    }

    /// Verifica se está anexado a uma sessão
    pub fn is_attached(&self) -> bool {
        self.current_session.is_some()
    }

    /// Desconecta do servidor
    pub fn disconnect(mut self) -> MultiplexerResult<()> {
        // Se estiver anexado, desanexa primeiro
        if self.is_attached() {
            let _ = self.detach();
        }

        let _ = self.send_request(MessageType::Quit);
        Ok(())
    }

    /// Envia uma requisição e aguarda resposta
    fn send_request(&mut self, msg_type: MessageType) -> MultiplexerResult<Message> {
        let id = self.message_counter.fetch_add(1, Ordering::SeqCst);
        let message = Message::new(id, msg_type);

        // Serializa e envia
        let json = message.encode().map_err(MultiplexerError::Serialization)?;

        self.send_line(&json)?;

        // Aguarda resposta
        let response_json = self.recv_line()?;
        let response = Message::decode(&response_json).map_err(MultiplexerError::Serialization)?;

        Ok(response)
    }

    /// Envia uma linha
    fn send_line(&mut self, data: &str) -> MultiplexerResult<()> {
        self.stream
            .write_all(data.as_bytes())
            .map_err(MultiplexerError::Io)?;
        self.stream.write_all(b"\n").map_err(MultiplexerError::Io)?;
        self.stream.flush().map_err(MultiplexerError::Io)
    }

    /// Recebe uma linha
    ///
    /// Acumula bytes (não texto) até achar `\n`: um caractere UTF-8 partido
    /// entre duas leituras não pode virar U+FFFD, e o que vier depois da
    /// quebra de linha fica guardado para a próxima mensagem.
    fn recv_line(&mut self) -> MultiplexerResult<String> {
        let mut buffer = vec![0u8; 4096];

        loop {
            if let Some(pos) = self.pending.iter().position(|&byte| byte == b'\n') {
                let line: Vec<u8> = self.pending.drain(..=pos).collect();
                return Ok(String::from_utf8_lossy(&line).trim_end().to_string());
            }

            match self.stream.read(&mut buffer) {
                Ok(0) => break,
                Ok(n) => self.pending.extend_from_slice(&buffer[..n]),
                Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => {
                    // Timeout ou não-blocking, continua
                    if !self.pending.is_empty() {
                        break;
                    }
                    std::thread::sleep(std::time::Duration::from_millis(10));
                }
                Err(e) => return Err(MultiplexerError::Io(e)),
            }
        }

        let line = std::mem::take(&mut self.pending);
        Ok(String::from_utf8_lossy(&line).trim_end().to_string())
    }
}

/// Parseia um ID de sessão a partir de string
fn parse_session_id(s: &str) -> MultiplexerResult<SessionId> {
    // Extrai o UUID do formato curto
    let uuid = s
        .parse::<uuid::Uuid>()
        .map_err(|_| MultiplexerError::ServerError(format!("Invalid session ID: {}", s)))?;
    Ok(SessionId(uuid))
}

/// Mapeia código de erro do protocolo para erro do cliente
fn map_error(code: ErrorCode, message: String) -> MultiplexerError {
    match code {
        ErrorCode::SessionNotFound => MultiplexerError::SessionNotFound(message),
        ErrorCode::SessionLocked => MultiplexerError::SessionLocked(message),
        ErrorCode::SessionNotAttachable => MultiplexerError::SessionNotAttachable(message),
        ErrorCode::NotAttached => MultiplexerError::NotAttached,
        ErrorCode::PermissionDenied => {
            MultiplexerError::ServerError(format!("Permission denied: {}", message))
        }
        _ => MultiplexerError::ServerError(format!("{:?}: {}", code, message)),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_session_id() {
        let uuid_str = "550e8400-e29b-41d4-a716-446655440000";
        let id = parse_session_id(uuid_str).unwrap();
        assert_eq!(id.0.to_string(), uuid_str);
    }

    #[test]
    fn test_parse_invalid_session_id() {
        let result = parse_session_id("invalid");
        assert!(result.is_err());
    }

    #[test]
    fn test_map_error_session_not_found() {
        let err = map_error(ErrorCode::SessionNotFound, "test session".to_string());
        assert!(matches!(err, MultiplexerError::SessionNotFound(_)));
    }

    #[test]
    fn test_map_error_session_locked() {
        let err = map_error(ErrorCode::SessionLocked, "locked".to_string());
        assert!(matches!(err, MultiplexerError::SessionLocked(_)));
    }

    #[test]
    fn test_map_error_not_attached() {
        let err = map_error(ErrorCode::NotAttached, "".to_string());
        assert!(matches!(err, MultiplexerError::NotAttached));
    }

    #[test]
    fn test_map_error_other() {
        let err = map_error(ErrorCode::ServerError, "server down".to_string());
        assert!(matches!(err, MultiplexerError::ServerError(_)));
    }

    use std::io::{BufRead, BufReader};
    use std::net::TcpListener;
    use std::thread::JoinHandle;

    const SESSION_ID: &str = "550e8400-e29b-41d4-a716-446655440000";

    /// Servidor falso no loopback: para cada resposta, lê uma requisição e
    /// escreve os bytes da resposta. Devolve os tipos das requisições lidas.
    fn fake_server(replies: Vec<Vec<u8>>) -> (String, JoinHandle<Vec<MessageType>>) {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let addr = listener.local_addr().unwrap().to_string();
        let handle = std::thread::spawn(move || {
            let (stream, _) = listener.accept().unwrap();
            let mut reader = BufReader::new(stream.try_clone().unwrap());
            let mut writer = stream;
            let mut requests = Vec::new();
            for reply in replies {
                let mut line = String::new();
                if reader.read_line(&mut line).unwrap() == 0 {
                    break;
                }
                requests.push(Message::decode(line.trim_end()).unwrap().msg_type);
                writer.write_all(&reply).unwrap();
                writer.flush().unwrap();
            }
            requests
        });
        (addr, handle)
    }

    fn reply(msg_type: MessageType) -> Vec<u8> {
        let mut bytes = Message::new(0, msg_type).encode().unwrap().into_bytes();
        bytes.push(b'\n');
        bytes
    }

    fn error_reply(code: ErrorCode) -> Vec<u8> {
        reply(MessageType::Error {
            code,
            message: "falhou".to_string(),
        })
    }

    fn session_info(name: &str) -> SessionInfo {
        SessionInfo {
            id: SESSION_ID.to_string(),
            name: name.to_string(),
            state: "running".to_string(),
            clients: 1,
            size: (80, 24),
            working_dir: "/tmp".to_string(),
            created_at: "agora".to_string(),
            idle_seconds: 0,
            flags: super::super::protocol::SessionFlagsInfo {
                destroy_on_exit: false,
                attachable: true,
                shared: false,
                locked: false,
            },
            app: None,
        }
    }

    #[test]
    fn test_connect_to_reports_refused_connection() {
        let addr = {
            let listener = TcpListener::bind("127.0.0.1:0").unwrap();
            listener.local_addr().unwrap().to_string()
        };
        match SessionClient::connect_to(&addr) {
            Err(MultiplexerError::ServerError(message)) => {
                assert!(message.starts_with("Failed to connect"), "{message}")
            }
            Ok(_) => panic!("não deveria conectar"),
            Err(other) => panic!("erro inesperado: {other:?}"),
        }
    }

    #[test]
    fn test_session_lifecycle_against_fake_server() {
        let (addr, server) = fake_server(vec![
            reply(MessageType::Pong),
            reply(MessageType::SessionCreated {
                session_id: SESSION_ID.to_string(),
                name: "dev".to_string(),
            }),
            reply(MessageType::Attached {
                session_id: SESSION_ID.to_string(),
                name: "dev".to_string(),
            }),
            reply(MessageType::Output { data: vec![] }),
            reply(MessageType::Output { data: vec![] }),
            reply(MessageType::SessionList {
                sessions: vec![session_info("dev")],
            }),
            reply(MessageType::SessionRenamed {
                session_id: SESSION_ID.to_string(),
                new_name: "prod".to_string(),
            }),
            reply(MessageType::Detached {
                session_id: SESSION_ID.to_string(),
            }),
            reply(MessageType::SessionDestroyed {
                session_id: SESSION_ID.to_string(),
            }),
            reply(MessageType::Pong),
        ]);

        let mut client = SessionClient::connect_to(&addr).unwrap();
        assert!(client.ping().unwrap());
        let (id, name) = client
            .create_session(Some("dev".to_string()), (80, 24))
            .unwrap();
        assert_eq!(
            (id.0.to_string(), name.as_str()),
            (SESSION_ID.to_string(), "dev")
        );
        assert!(!client.is_attached());

        let (attached, _) = client.attach(SESSION_ID, false).unwrap();
        assert_eq!(client.current_session(), Some(attached));
        assert!(client.is_attached());

        client.send_input(b"ls\r").unwrap();
        client.resize(100, 30).unwrap();
        let sessions = client.list_sessions().unwrap();
        assert_eq!(sessions, vec![session_info("dev")]);
        client
            .rename_session(SESSION_ID, "prod".to_string())
            .unwrap();
        client.detach().unwrap();
        assert!(!client.is_attached());
        client.destroy_session(SESSION_ID).unwrap();
        client.disconnect().unwrap();

        let requests = server.join().unwrap();
        assert!(matches!(requests[0], MessageType::Ping));
        assert!(matches!(
            requests[1],
            MessageType::CreateSession { size: (80, 24), .. }
        ));
        assert!(matches!(
            &requests[2],
            MessageType::Attach {
                read_only: false,
                ..
            }
        ));
        assert!(matches!(&requests[3], MessageType::Input { data } if data == b"ls\r"));
        assert!(matches!(
            requests[4],
            MessageType::Resize {
                cols: 100,
                rows: 30
            }
        ));
        assert!(matches!(requests[9], MessageType::Quit));
    }

    #[test]
    fn test_error_and_unexpected_responses() {
        let (addr, server) = fake_server(vec![
            reply(MessageType::Detach),
            error_reply(ErrorCode::SessionAlreadyExists),
            reply(MessageType::Pong),
            reply(MessageType::SessionCreated {
                session_id: "nao-e-uuid".to_string(),
                name: "x".to_string(),
            }),
            error_reply(ErrorCode::SessionNotFound),
            reply(MessageType::Pong),
            error_reply(ErrorCode::NotAttached),
            error_reply(ErrorCode::PermissionDenied),
            reply(MessageType::Pong),
            error_reply(ErrorCode::ServerError),
            reply(MessageType::Pong),
            error_reply(ErrorCode::SessionLocked),
            reply(MessageType::Pong),
            error_reply(ErrorCode::InvalidSessionId),
            reply(MessageType::Pong),
            error_reply(ErrorCode::NotAttached),
            error_reply(ErrorCode::InvalidMessage),
        ]);
        let mut client = SessionClient::connect_to(&addr).unwrap();

        assert!(!client.ping().unwrap());
        let unexpected = |result: MultiplexerResult<()>| match result {
            Err(MultiplexerError::ServerError(message)) => {
                assert_eq!(message, "Unexpected response")
            }
            other => panic!("esperava resposta inesperada, veio {:?}", other),
        };

        match client.create_session(None, (80, 24)) {
            Err(MultiplexerError::ServerError(message)) => {
                assert_eq!(message, "SessionAlreadyExists: falhou")
            }
            other => panic!("{:?}", other),
        }
        unexpected(client.create_session(None, (80, 24)).map(|_| ()));
        match client.create_session(None, (80, 24)) {
            Err(MultiplexerError::ServerError(message)) => {
                assert!(message.contains("Invalid session ID"), "{message}")
            }
            other => panic!("{:?}", other),
        }

        assert!(matches!(
            client.attach(SESSION_ID, true),
            Err(MultiplexerError::SessionNotFound(_))
        ));
        unexpected(client.attach(SESSION_ID, true).map(|_| ()));

        // Já desanexado não é erro; outros erros são
        client.detach().unwrap();
        match client.detach() {
            Err(MultiplexerError::ServerError(message)) => {
                assert_eq!(message, "Permission denied: falhou")
            }
            other => panic!("{:?}", other),
        }
        unexpected(client.detach());

        assert!(matches!(
            client.list_sessions(),
            Err(MultiplexerError::ServerError(_))
        ));
        unexpected(client.list_sessions().map(|_| ()));

        assert!(matches!(
            client.rename_session(SESSION_ID, "x".to_string()),
            Err(MultiplexerError::SessionLocked(_))
        ));
        unexpected(client.rename_session(SESSION_ID, "x".to_string()));

        assert!(matches!(
            client.destroy_session(SESSION_ID),
            Err(MultiplexerError::ServerError(_))
        ));
        unexpected(client.destroy_session(SESSION_ID));

        assert!(matches!(
            client.send_input(b"x"),
            Err(MultiplexerError::NotAttached)
        ));
        assert!(matches!(
            client.resize(1, 1),
            Err(MultiplexerError::ServerError(_))
        ));
        drop(client);
        server.join().unwrap();
    }

    #[test]
    fn test_map_error_remaining_codes() {
        assert!(matches!(
            map_error(ErrorCode::SessionNotAttachable, "x".to_string()),
            MultiplexerError::SessionNotAttachable(_)
        ));
        match map_error(ErrorCode::PermissionDenied, "sem acesso".to_string()) {
            MultiplexerError::ServerError(message) => {
                assert_eq!(message, "Permission denied: sem acesso")
            }
            other => panic!("{:?}", other),
        }
    }

    /// Regressão: duas respostas no mesmo pacote viravam uma "linha" só e a
    /// decodificação falhava; um caractere UTF-8 partido entre leituras
    /// virava U+FFFD.
    #[test]
    fn test_recv_line_keeps_leftover_and_multibyte_chars() {
        // Nome cujo "é" cai exatamente na fronteira de 4096 bytes da leitura
        let mut padding = 4000;
        let long_name = loop {
            let name = format!("{}é", "a".repeat(padding));
            let encoded = Message::new(
                0,
                MessageType::SessionList {
                    sessions: vec![session_info(&name)],
                },
            )
            .encode()
            .unwrap();
            let position = encoded.find('é').unwrap();
            if position == 4095 {
                break name;
            }
            padding = padding + 4095 - position;
        };

        let mut both = reply(MessageType::Pong);
        both.extend(reply(MessageType::Pong));
        let (addr, server) = fake_server(vec![
            both,
            reply(MessageType::SessionList {
                sessions: vec![session_info(&long_name)],
            }),
        ]);
        let mut client = SessionClient::connect_to(&addr).unwrap();
        // A primeira requisição recebe as duas respostas de uma vez
        assert!(client.ping().unwrap());
        // A segunda resposta já estava no buffer
        let leftover = client.recv_line().unwrap();
        assert!(matches!(
            Message::decode(&leftover).unwrap().msg_type,
            MessageType::Pong
        ));

        let sessions = client.list_sessions().unwrap();
        assert_eq!(sessions[0].name, long_name);
        drop(client);
        server.join().unwrap();
    }

    #[test]
    fn test_closed_connection_yields_decode_error() {
        let (addr, server) = fake_server(vec![Vec::new()]);
        let mut client = SessionClient::connect_to(&addr).unwrap();
        // O servidor lê o ping, não responde nada e fecha a conexão
        assert!(matches!(
            client.ping(),
            Err(MultiplexerError::Serialization(_))
        ));
        server.join().unwrap();
    }
}
