mod admin;
mod auth;
mod config;
mod db;
mod error;
mod site;

use axum::{
    extract::State,
    response::{Html, IntoResponse, Redirect},
    routing::{delete, get, post},
    Router,
};
use std::net::SocketAddr;
use tower_http::limit::RequestBodyLimitLayer;

use config::Config;
use db::Db;

#[derive(Clone)]
pub struct AppState {
    pub config: Config,
    pub db: Db,
}


// --- Template rendering ---

#[derive(askama::Template)]
#[template(path = "login.html")]
struct LoginTemplate {
    error: Option<String>,
}

#[derive(askama::Template)]
#[template(path = "register.html")]
struct RegisterTemplate {
    error: Option<String>,
}

#[derive(askama::Template)]
#[template(path = "dashboard.html")]
struct DashboardTemplate {
    username: String,
}

#[derive(askama::Template)]
#[template(path = "admin.html")]
struct AdminTemplate {}

// --- Page handlers ---

async fn page_index(
    State(state): State<AppState>,
    headers: axum::http::HeaderMap,
) -> impl IntoResponse {
    // If user is already logged in, redirect to dashboard
    if let Some(cookie) = headers.get(axum::http::header::COOKIE) {
        if let Ok(cookie_str) = cookie.to_str() {
            if let Some(token) = cookie_str.split(';').find_map(|c| {
                let c = c.trim();
                c.starts_with("token=").then(|| c.trim_start_matches("token="))
            }) {
                if let Ok(claims) = auth::decode_token(token, &state.config.jwt_secret) {
                    if claims.is_admin {
                        return Redirect::to("/admin").into_response();
                    }
                    return Redirect::to("/dashboard").into_response();
                }
            }
        }
    }

    let template = LoginTemplate { error: None };
    Html(template.to_string()).into_response()
}

async fn page_register() -> impl IntoResponse {
    let template = RegisterTemplate { error: None };
    Html(template.to_string())
}

async fn page_dashboard(user: auth::AuthUser) -> impl IntoResponse {
    let template = DashboardTemplate {
        username: user.username,
    };
    Html(template.to_string())
}

async fn page_admin(_admin: auth::AdminUser) -> impl IntoResponse {
    let template = AdminTemplate {};
    Html(template.to_string())
}

// --- Static assets for management UI ---
async fn serve_static(
    axum::extract::Path(filename): axum::extract::Path<String>,
) -> impl IntoResponse {
    let content = match filename.as_str() {
        "app.js" => Some((
            "application/javascript; charset=utf-8",
            include_str!("../static/app.js"),
        )),
        _ => None,
    };

    match content {
        Some((mime, body)) => (
            axum::http::StatusCode::OK,
            [(axum::http::header::CONTENT_TYPE, mime)],
            body.to_string(),
        )
            .into_response(),
        None => axum::http::StatusCode::NOT_FOUND.into_response(),
    }
}

#[tokio::main]
async fn main() {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| "simple_pages=info,tower_http=info".parse().unwrap()),
        )
        .init();

    let config = Config::from_env();

    // Ensure directories exist
    std::fs::create_dir_all(config.sites_dir()).expect("Failed to create sites directory");

    let db = Db::open(&config.db_path()).expect("Failed to open database");

    let state = AppState {
        config: config.clone(),
        db,
    };

    let app = Router::new()
        // Pages
        .route("/", get(page_index))
        .route("/register", get(page_register))
        .route("/dashboard", get(page_dashboard))
        .route("/admin", get(page_admin))
        // Static assets for management UI
        .route("/static/{filename}", get(serve_static))
        // Auth API
        .route("/api/auth/register", post(auth::api_register))
        .route("/api/auth/login", post(auth::api_login))
        .route("/api/auth/logout", get(auth::api_logout))
        // Site API
        .route("/api/site", get(site::get_site_info))
        .route("/api/site/upload", post(site::upload_site))
        .route("/api/site", delete(site::delete_site))
        // Admin API
        .route("/api/admin/codes", get(admin::list_codes))
        .route("/api/admin/codes", post(admin::generate_codes))
        .route("/api/admin/codes/{code}", delete(admin::revoke_code))
        // User sites — must be last (catch-all)
        .route("/{username}/", get(site::serve_user_site_index))
        .route("/{username}/{*path}", get(site::serve_user_site))
        .layer(RequestBodyLimitLayer::new(
            config.max_upload_bytes as usize + 1024, // small overhead for multipart headers
        ))
        .with_state(state);

    let addr: SocketAddr = config.listen_addr.parse().expect("Invalid listen address");
    tracing::info!("Listening on {addr}");

    let listener = tokio::net::TcpListener::bind(addr)
        .await
        .expect("Failed to bind");

    axum::serve(listener, app).await.expect("Server error");
}
