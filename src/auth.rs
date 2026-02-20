use argon2::{
    password_hash::{rand_core::OsRng, PasswordHash, PasswordHasher, PasswordVerifier, SaltString},
    Argon2,
};
use axum::{
    extract::{FromRequestParts, State},
    http::{header, request::Parts, StatusCode},
    response::{IntoResponse, Redirect, Response},
    Form,
};
use jsonwebtoken::{decode, encode, DecodingKey, EncodingKey, Header, Validation};
use serde::{Deserialize, Serialize};

use crate::error::AppError;
use crate::AppState;

#[derive(Debug, Serialize, Deserialize)]
pub struct Claims {
    pub sub: String, // username
    pub is_admin: bool,
    pub exp: usize,
}

pub fn hash_password(password: &str) -> Result<String, AppError> {
    let salt = SaltString::generate(&mut OsRng);
    let argon2 = Argon2::default();
    let hash = argon2
        .hash_password(password.as_bytes(), &salt)
        .map_err(|e| AppError::Internal(format!("Failed to hash password: {e}")))?;
    Ok(hash.to_string())
}

pub fn verify_password(password: &str, hash: &str) -> Result<bool, AppError> {
    let parsed = PasswordHash::new(hash)
        .map_err(|e| AppError::Internal(format!("Invalid password hash: {e}")))?;
    Ok(Argon2::default()
        .verify_password(password.as_bytes(), &parsed)
        .is_ok())
}

pub fn create_token(username: &str, is_admin: bool, secret: &str) -> Result<String, AppError> {
    let expiration = chrono_exp_24h();
    let claims = Claims {
        sub: username.to_string(),
        is_admin,
        exp: expiration,
    };
    let token = encode(
        &Header::default(),
        &claims,
        &EncodingKey::from_secret(secret.as_bytes()),
    )?;
    Ok(token)
}

pub fn decode_token(token: &str, secret: &str) -> Result<Claims, AppError> {
    let data = decode::<Claims>(
        token,
        &DecodingKey::from_secret(secret.as_bytes()),
        &Validation::default(),
    )?;
    Ok(data.claims)
}

fn chrono_exp_24h() -> usize {
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_secs();
    (now + 86400) as usize
}

// Extractor for authenticated user
pub struct AuthUser {
    pub username: String,
    pub is_admin: bool,
}

impl<S> FromRequestParts<S> for AuthUser
where
    S: Send + Sync,
    AppState: FromRef<S>,
{
    type Rejection = Response;

    async fn from_request_parts(parts: &mut Parts, state: &S) -> Result<Self, Self::Rejection> {
        let app_state = AppState::from_ref(state);

        // Try to get token from cookie
        let cookie_header = parts
            .headers
            .get(header::COOKIE)
            .and_then(|v| v.to_str().ok())
            .unwrap_or("");

        let token = cookie_header
            .split(';')
            .find_map(|cookie| {
                let cookie = cookie.trim();
                if cookie.starts_with("token=") {
                    Some(cookie.trim_start_matches("token="))
                } else {
                    None
                }
            });

        let token = match token {
            Some(t) => t,
            None => return Err(Redirect::to("/").into_response()),
        };

        match decode_token(token, &app_state.config.jwt_secret) {
            Ok(claims) => Ok(AuthUser {
                username: claims.sub,
                is_admin: claims.is_admin,
            }),
            Err(_) => Err(Redirect::to("/").into_response()),
        }
    }
}

// Extractor for admin user
#[allow(dead_code)]
pub struct AdminUser {
    pub username: String,
}

impl<S> FromRequestParts<S> for AdminUser
where
    S: Send + Sync,
    AppState: FromRef<S>,
{
    type Rejection = Response;

    async fn from_request_parts(parts: &mut Parts, state: &S) -> Result<Self, Self::Rejection> {
        let auth = AuthUser::from_request_parts(parts, state).await?;
        if !auth.is_admin {
            return Err(AppError::Forbidden("Admin access required".to_string()).into_response());
        }
        Ok(AdminUser {
            username: auth.username,
        })
    }
}

use axum::extract::FromRef;

// --- Route handlers ---

#[derive(Deserialize)]
pub struct RegisterForm {
    pub username: String,
    pub password: String,
    pub invite_code: String,
}

#[derive(Deserialize)]
pub struct LoginForm {
    pub username: String,
    pub password: String,
}

pub async fn api_register(
    State(state): State<AppState>,
    Form(form): Form<RegisterForm>,
) -> Result<Response, AppError> {
    let username = form.username.trim().to_lowercase();

    // Validate username
    if username.is_empty() || username.len() > 32 {
        return Err(AppError::BadRequest("Username must be 1-32 characters".to_string()));
    }
    if !username.chars().all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_') {
        return Err(AppError::BadRequest(
            "Username may only contain letters, numbers, hyphens, and underscores".to_string(),
        ));
    }

    // Reserved names
    let reserved = ["admin", "api", "static", "dashboard", "register", "login", "logout"];
    if reserved.contains(&username.as_str()) {
        return Err(AppError::BadRequest("This username is reserved".to_string()));
    }

    // Hash password and register (validates invite code + creates user atomically)
    let password_hash = hash_password(&form.password)?;
    state.db.register_user(&username, &password_hash, &form.invite_code)?;

    // Create site directory
    let site_dir = state.config.sites_dir().join(&username);
    std::fs::create_dir_all(&site_dir)?;

    // Create token and set cookie
    let token = create_token(&username, false, &state.config.jwt_secret)?;
    let cookie = format!("token={token}; Path=/; HttpOnly; SameSite=Lax; Max-Age=86400");

    Ok((
        StatusCode::SEE_OTHER,
        [
            (header::SET_COOKIE, cookie),
            (header::LOCATION, "/dashboard".to_string()),
        ],
    )
        .into_response())
}

pub async fn api_login(
    State(state): State<AppState>,
    Form(form): Form<LoginForm>,
) -> Result<Response, AppError> {
    let username = form.username.trim().to_lowercase();

    // Check admin login
    if username == "admin" {
        if form.password != state.config.admin_password {
            return Err(AppError::Unauthorized("Invalid credentials".to_string()));
        }
        let token = create_token("admin", true, &state.config.jwt_secret)?;
        let cookie = format!("token={token}; Path=/; HttpOnly; SameSite=Lax; Max-Age=86400");
        return Ok((
            StatusCode::SEE_OTHER,
            [
                (header::SET_COOKIE, cookie),
                (header::LOCATION, "/admin".to_string()),
            ],
        )
            .into_response());
    }

    let user = state
        .db
        .get_user_by_username(&username)?
        .ok_or_else(|| AppError::Unauthorized("Invalid credentials".to_string()))?;

    if !verify_password(&form.password, &user.password_hash)? {
        return Err(AppError::Unauthorized("Invalid credentials".to_string()));
    }

    let token = create_token(&username, user.is_admin, &state.config.jwt_secret)?;
    let cookie = format!("token={token}; Path=/; HttpOnly; SameSite=Lax; Max-Age=86400");

    Ok((
        StatusCode::SEE_OTHER,
        [
            (header::SET_COOKIE, cookie),
            (header::LOCATION, "/dashboard".to_string()),
        ],
    )
        .into_response())
}

pub async fn api_logout() -> Response {
    let cookie = "token=; Path=/; HttpOnly; SameSite=Lax; Max-Age=0";
    (
        StatusCode::SEE_OTHER,
        [
            (header::SET_COOKIE, cookie.to_string()),
            (header::LOCATION, "/".to_string()),
        ],
    )
        .into_response()
}
