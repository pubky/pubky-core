// src/admin/routes/info.rs

use super::super::app_state::AppState;
use crate::persistence::lmdb::tables::signup_tokens::SignupToken;
use crate::shared::HttpResult;
use axum::{extract::State, http::StatusCode, Json};
use serde::Serialize;

#[derive(Serialize)]
pub(crate) struct InfoResponse {
    num_users: u64,
    num_disabled_users: u64,
    total_disk_used_mb: f64,
    num_signup_codes: u64,
    num_unused_signup_codes: u64,
}

/// Return summary statistics about the homeserver.
pub async fn info(State(state): State<AppState>) -> HttpResult<(StatusCode, Json<InfoResponse>)> {
    // Read-only transaction
    let rtxn = state.db.env.read_txn()?;

    // Count users, disabled flag, and accumulate usage
    let mut num_users = 0;
    let mut num_disabled_users = 0;
    let mut total_bytes = 0u64;
    let mut users_iter = state.db.tables.users.iter(&rtxn)?;
    while let Some(Ok((_pk, user))) = users_iter.next() {
        num_users += 1;
        if user.disabled {
            num_disabled_users += 1;
        }
        total_bytes = total_bytes.saturating_add(user.used_bytes);
    }

    // Count signup tokens and unused ones
    let mut num_signup_codes = 0;
    let mut num_unused_signup_codes = 0;
    let mut tokens_iter = state.db.tables.signup_tokens.iter(&rtxn)?;
    while let Some(Ok((_token_str, bytes))) = tokens_iter.next() {
        num_signup_codes += 1;
        let tok = SignupToken::deserialize(&bytes);
        if !tok.is_used() {
            num_unused_signup_codes += 1;
        }
    }

    // Build response
    let body = InfoResponse {
        num_users,
        num_disabled_users,
        total_disk_used_mb: (total_bytes as f64) / (1024.0 * 1024.0),
        num_signup_codes,
        num_unused_signup_codes,
    };

    Ok((StatusCode::OK, Json(body)))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::admin::app_state::AppState;
    use crate::persistence::lmdb::LmDB;
    use axum::extract::State;
    use axum::http::StatusCode;
    use pkarr::Keypair;

    #[tokio::test]
    async fn test_info_counts() {
        // Setup test DB
        let mut db = LmDB::test();
        let key1 = Keypair::random().public_key();
        let key2 = Keypair::random().public_key();

        // 1) Create both users
        {
            let mut wtxn = db.env.write_txn().unwrap();
            db.create_user(&key1, &mut wtxn).unwrap();
            db.create_user(&key2, &mut wtxn).unwrap();
            wtxn.commit().unwrap();
        }

        // 2) Modify usage and disabled flags
        {
            let mut wtxn = db.env.write_txn().unwrap();
            // User1: enabled, 1 MB
            let mut user1 = db.get_user(&key1, &mut db.env.read_txn().unwrap()).unwrap();
            user1.used_bytes = 1024 * 1024;
            db.tables.users.put(&mut wtxn, &key1, &user1).unwrap();

            // User2: disabled, 0.5 MB
            let mut user2 = db.get_user(&key2, &mut db.env.read_txn().unwrap()).unwrap();
            user2.disabled = true;
            user2.used_bytes = 512 * 1024;
            db.tables.users.put(&mut wtxn, &key2, &user2).unwrap();

            wtxn.commit().unwrap();
        }

        // 3) Create two signup tokens and consume one
        let code1 = db.generate_signup_token().unwrap();
        let _code2 = db.generate_signup_token().unwrap();
        db.validate_and_consume_signup_token(&code1, &key1).unwrap();

        // 4) Invoke handler
        let state = AppState::new(db);
        let (status, Json(info)) = info(State(state)).await.unwrap();
        assert_eq!(status, StatusCode::OK);
        assert_eq!(info.num_users, 2);
        assert_eq!(info.num_disabled_users, 1);
        // 1 MB + 0.5 MB = 1.5 MB
        assert!((info.total_disk_used_mb - 1.5).abs() < 1e-6);
        assert_eq!(info.num_signup_codes, 2);
        assert_eq!(info.num_unused_signup_codes, 1);
    }
}
