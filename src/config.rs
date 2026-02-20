use std::env;
use std::path::PathBuf;

#[derive(Clone)]
pub struct Config {
    pub data_dir: PathBuf,
    pub disk_quota_bytes: u64,
    pub max_upload_bytes: u64,
    pub admin_password: String,
    pub jwt_secret: String,
    pub listen_addr: String,
}

impl Config {
    pub fn from_env() -> Self {
        let disk_quota_mb: u64 = env::var("DISK_QUOTA_MB")
            .unwrap_or_else(|_| "50".to_string())
            .parse()
            .expect("DISK_QUOTA_MB must be a number");

        let max_upload_mb: u64 = env::var("MAX_UPLOAD_MB")
            .unwrap_or_else(|_| "50".to_string())
            .parse()
            .expect("MAX_UPLOAD_MB must be a number");

        let admin_password =
            env::var("ADMIN_PASSWORD").unwrap_or_else(|_| "admin".to_string());

        let jwt_secret = env::var("JWT_SECRET")
            .unwrap_or_else(|_| "change-me-in-production".to_string());

        let data_dir = PathBuf::from(env::var("DATA_DIR").unwrap_or_else(|_| "/data".to_string()));

        let listen_addr =
            env::var("LISTEN_ADDR").unwrap_or_else(|_| "0.0.0.0:8080".to_string());

        Self {
            data_dir,
            disk_quota_bytes: disk_quota_mb * 1024 * 1024,
            max_upload_bytes: max_upload_mb * 1024 * 1024,
            admin_password,
            jwt_secret,
            listen_addr,
        }
    }

    pub fn db_path(&self) -> PathBuf {
        self.data_dir.join("pages.db")
    }

    pub fn sites_dir(&self) -> PathBuf {
        self.data_dir.join("sites")
    }
}
