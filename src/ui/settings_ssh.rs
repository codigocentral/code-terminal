//! Formulário de host SSH dentro de Configurações › SSH (#32): criar e
//! editar blocos `Host` do `~/.ssh/config`.
//!
//! A tela só monta os campos; quem grava o arquivo é o app
//! (`ssh::config_edit`), com backup e escrita atômica.

use crate::ssh::config_edit::HostEntry;
use crate::ui::i18n::{tr, Text};
use crate::ui::line_edit::{LineEdit, LineEditKey};

/// Campos do formulário, na ordem das linhas.
pub const FORM_FIELDS: [Text; 5] = [
    Text::SshFieldAlias,
    Text::SshFieldHostName,
    Text::SshFieldUserLabel,
    Text::SshFieldPortLabel,
    Text::SshFieldIdentity,
];

/// Host sendo criado ou editado.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SshForm {
    /// Alias original (`None` = host novo).
    pub original: Option<String>,
    pub fields: [LineEdit; 5],
    /// Mensagem do último Salvar recusado.
    pub error: Option<String>,
}

impl SshForm {
    /// Formulário para `entry` (ou vazio, para um host novo).
    pub fn new(entry: Option<&HostEntry>) -> Self {
        let value = |v: Option<&String>| v.map(String::as_str).unwrap_or("").to_string();
        let texts = match entry {
            Some(e) => [
                e.alias.clone(),
                value(e.host_name.as_ref()),
                value(e.user.as_ref()),
                e.port.map(|p| p.to_string()).unwrap_or_default(),
                value(e.identity_file.as_ref()),
            ],
            None => Default::default(),
        };
        Self {
            original: entry.map(|e| e.alias.clone()),
            fields: texts.map(|text| {
                let mut edit = LineEdit::new(&text);
                // Campo começa com o cursor no fim, sem seleção.
                edit.apply(LineEditKey::End);
                edit
            }),
            error: None,
        }
    }

    /// Tecla num campo de texto.
    pub fn edit(&mut self, field: usize, key: LineEditKey) {
        if let Some(edit) = self.fields.get_mut(field) {
            edit.apply(key);
            self.error = None;
        }
    }

    /// Os campos como `HostEntry`, ou a mensagem do que está errado.
    pub fn entry(&self) -> Result<HostEntry, String> {
        let text = |i: usize| self.fields[i].text().trim().to_string();
        let optional = |i: usize| Some(text(i)).filter(|v| !v.is_empty());
        let port = match optional(3) {
            None => None,
            Some(port) => match port.parse::<u16>() {
                Ok(p) if p > 0 => Some(p),
                _ => return Err(tr(Text::SshInvalidPort).to_string()),
            },
        };
        Ok(HostEntry {
            alias: text(0),
            host_name: optional(1),
            user: optional(2),
            port,
            identity_file: optional(4),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn form_round_trips_an_entry_and_validates_the_port() {
        let entry = HostEntry {
            alias: "prod".into(),
            host_name: Some("10.0.0.5".into()),
            user: Some("deploy".into()),
            port: Some(2222),
            identity_file: None,
        };
        let mut form = SshForm::new(Some(&entry));
        assert_eq!(form.original.as_deref(), Some("prod"));
        assert_eq!(form.entry().unwrap(), entry);
        // Digitar no fim do campo, sem apagar o que tinha.
        form.edit(2, LineEditKey::Char('2'));
        assert_eq!(form.entry().unwrap().user.as_deref(), Some("deploy2"));
        // Porta inválida.
        form.edit(3, LineEditKey::Char('x'));
        assert!(form.entry().is_err());
        // Campo vazio vira None.
        let mut empty = SshForm::new(None);
        assert_eq!(empty.original, None);
        for c in "novo".chars() {
            empty.edit(0, LineEditKey::Char(c));
        }
        assert_eq!(
            empty.entry().unwrap(),
            HostEntry {
                alias: "novo".into(),
                ..Default::default()
            }
        );
    }
}
