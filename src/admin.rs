use axum::{
    extract::{Path, State},
    Json,
};
use rand::Rng;

use crate::auth::AdminUser;
use crate::error::AppError;
use crate::AppState;

pub async fn list_codes(
    _admin: AdminUser,
    State(state): State<AppState>,
) -> Result<Json<serde_json::Value>, AppError> {
    let codes = state.db.list_invite_codes()?;
    Ok(Json(serde_json::json!({ "codes": codes })))
}

#[derive(serde::Deserialize)]
pub struct GenerateCodesRequest {
    #[serde(default = "default_count")]
    pub count: usize,
}

fn default_count() -> usize {
    1
}

pub async fn generate_codes(
    _admin: AdminUser,
    State(state): State<AppState>,
    Json(req): Json<GenerateCodesRequest>,
) -> Result<Json<serde_json::Value>, AppError> {
    let count = req.count.min(50); // cap at 50
    let mut codes = Vec::with_capacity(count);

    for _ in 0..count {
        let code = generate_invite_code();
        state.db.create_invite_code(&code)?;
        codes.push(code);
    }

    Ok(Json(serde_json::json!({ "codes": codes })))
}

pub async fn revoke_code(
    _admin: AdminUser,
    State(state): State<AppState>,
    Path(code): Path<String>,
) -> Result<Json<serde_json::Value>, AppError> {
    let deleted = state.db.delete_invite_code(&code)?;
    if !deleted {
        return Err(AppError::NotFound(
            "Code not found or already used".to_string(),
        ));
    }
    Ok(Json(serde_json::json!({ "success": true })))
}

fn generate_invite_code() -> String {
    let mut rng = rand::thread_rng();
    let chars: Vec<char> = "ABCDEFGHJKLMNPQRSTUVWXYZ23456789"
        .chars()
        .collect();
    let code: String = (0..8).map(|_| chars[rng.gen_range(0..chars.len())]).collect();
    format!("{}-{}", &code[..4], &code[4..])
}
