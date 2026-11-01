//! Contexte : erreurs communes avec une traduction HTTP unique.
//! Usage : le métier renvoie `AppError` sans connaître les détails des routes Axum.

use std::io;

use axum::{
    Json,
    http::StatusCode,
    response::{IntoResponse, Response},
};
use serde::Serialize;
use thiserror::Error;

/// Alias court utilisé dans toute la bibliothèque.
pub type AppResult<T> = Result<T, AppError>;

/// Catégories d'erreurs compréhensibles par les CLI et l'API.
#[derive(Debug, Error)]
pub enum AppError {
    #[error("requête invalide : {0}")]
    Validation(String),
    #[error("ressource introuvable : {0}")]
    NotFound(String),
    #[error("conflit : {0}")]
    Conflict(String),
    #[error("moteur de traitement : {0}")]
    Engine(String),
    #[error("stockage : {0}")]
    Storage(String),
    #[error("configuration : {0}")]
    Configuration(String),
}

impl AppError {
    /// Crée une erreur de validation sans répéter le nom de sa variante.
    pub fn validation(message: impl Into<String>) -> Self {
        Self::Validation(message.into())
    }

    /// Crée une erreur de ressource absente.
    pub fn not_found(message: impl Into<String>) -> Self {
        Self::NotFound(message.into())
    }

    /// Crée une erreur provenant d'un exécutable de traitement.
    pub fn engine(message: impl Into<String>) -> Self {
        Self::Engine(message.into())
    }

    /// Crée une erreur d'accès au stockage local.
    pub fn storage(message: impl Into<String>) -> Self {
        Self::Storage(message.into())
    }

    /// Crée une erreur de lecture ou de validation de la configuration.
    pub fn configuration(message: impl Into<String>) -> Self {
        Self::Configuration(message.into())
    }
}

impl From<io::Error> for AppError {
    /// Conserve le message système tout en le classant comme erreur de stockage.
    fn from(error: io::Error) -> Self {
        Self::Storage(error.to_string())
    }
}

/// Corps JSON stable renvoyé lorsqu'une route échoue.
#[derive(Serialize)]
struct ErrorBody {
    error: &'static str,
    message: String,
}

impl IntoResponse for AppError {
    /// Associe chaque erreur métier à un statut HTTP prévisible.
    fn into_response(self) -> Response {
        let (status, code) = match &self {
            Self::Validation(_) => (StatusCode::BAD_REQUEST, "validation_error"),
            Self::NotFound(_) => (StatusCode::NOT_FOUND, "not_found"),
            Self::Conflict(_) => (StatusCode::CONFLICT, "conflict"),
            Self::Engine(_) => (StatusCode::UNPROCESSABLE_ENTITY, "engine_error"),
            Self::Storage(_) | Self::Configuration(_) => {
                (StatusCode::INTERNAL_SERVER_ERROR, "internal_error")
            }
        };
        (
            status,
            Json(ErrorBody {
                error: code,
                message: self.to_string(),
            }),
        )
            .into_response()
    }
}
